//! Pure classification of a Windows toast notification into an incoming
//! Teams call (or not). Toast wording is not a stable contract across Teams
//! versions, so the phrase list is a heuristic and overridable — matching
//! primarily happens on the app id.

/// New Teams desktop client's AppUserModelId. Classic Teams is being
/// retired; we target new Teams only.
pub const TEAMS_AUMID: &str = "MSTeams_8wekyb3d8bbwe!MSTeams";

/// Default phrases (lowercased) that indicate a ringing call rather than a
/// chat/mention toast. Overridable via `CallClassifier::with_phrases`.
const DEFAULT_CALL_PHRASES: &[&str] = &["is calling you", "incoming call", "is calling"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingCall {
    pub caller: String,
}

pub struct CallClassifier {
    phrases: Vec<String>,
}

impl Default for CallClassifier {
    fn default() -> Self {
        CallClassifier {
            phrases: DEFAULT_CALL_PHRASES.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl CallClassifier {
    pub fn with_phrases(phrases: Vec<String>) -> Self {
        CallClassifier { phrases }
    }

    /// `toast_lines` are the toast's text elements in order (title first,
    /// body lines after), as read from `ToastGeneric`'s text bindings.
    pub fn classify(&self, aumid: &str, toast_lines: &[String]) -> Option<IncomingCall> {
        if aumid != TEAMS_AUMID {
            return None;
        }
        self.classify_lines(toast_lines)
    }

    /// Same phrase-matching heuristic as `classify`, without an AUMID check —
    /// for sources that read Teams' own window/UIA text rather than an OS
    /// toast (which carries no AUMID), e.g. `window_win::WindowCallSource`.
    pub fn classify_lines(&self, lines: &[String]) -> Option<IncomingCall> {
        let joined = lines.join(" ").to_lowercase();
        let is_call = self.phrases.iter().any(|p| joined.contains(p.as_str()));
        if !is_call {
            return None;
        }

        let caller = lines
            .first()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Unknown caller".to_string());

        Some(IncomingCall { caller })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_incoming_call_toast() {
        let c = CallClassifier::default();
        let result = c.classify(
            TEAMS_AUMID,
            &["Sarah Lee".to_string(), "is calling you".to_string()],
        );
        assert_eq!(
            result,
            Some(IncomingCall {
                caller: "Sarah Lee".to_string()
            })
        );
    }

    #[test]
    fn ignores_non_teams_aumid() {
        let c = CallClassifier::default();
        let result = c.classify(
            "SomeOtherApp_123!App",
            &["Sarah Lee".to_string(), "is calling you".to_string()],
        );
        assert_eq!(result, None);
    }

    #[test]
    fn ignores_teams_chat_message_toast() {
        let c = CallClassifier::default();
        let result = c.classify(
            TEAMS_AUMID,
            &["Sarah Lee".to_string(), "Hey, are you around?".to_string()],
        );
        assert_eq!(result, None);
    }

    #[test]
    fn custom_phrases_are_honored() {
        let c = CallClassifier::with_phrases(vec!["te está llamando".to_string()]);
        let result = c.classify(
            TEAMS_AUMID,
            &["Sarah Lee".to_string(), "te está llamando".to_string()],
        );
        assert_eq!(
            result,
            Some(IncomingCall {
                caller: "Sarah Lee".to_string()
            })
        );
    }

    #[test]
    fn empty_first_line_falls_back_to_unknown_caller() {
        let c = CallClassifier::default();
        let result = c.classify(TEAMS_AUMID, &["".to_string(), "is calling you".to_string()]);
        assert_eq!(
            result,
            Some(IncomingCall {
                caller: "Unknown caller".to_string()
            })
        );
    }

    #[test]
    fn no_lines_yields_none() {
        let c = CallClassifier::default();
        assert_eq!(c.classify(TEAMS_AUMID, &[]), None);
    }

    #[test]
    fn classify_lines_has_no_aumid_gate() {
        let c = CallClassifier::default();
        let result = c.classify_lines(&["Sarah Lee".to_string(), "is calling you".to_string()]);
        assert_eq!(
            result,
            Some(IncomingCall {
                caller: "Sarah Lee".to_string()
            })
        );
    }
}
