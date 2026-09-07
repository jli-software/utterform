//! The prompts Utterform ships with, and the user's replacements for them.
//!
//! Every shipped prompt is a starting point rather than a fixture: Settings can
//! rewrite any of them, and what the user typed is kept in
//! [`AppSettings::action_overrides`](crate::domain::AppSettings) beside the id
//! it belongs to. The defaults live here alone, so the interface can show them
//! without a second copy drifting out of step.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The action that is delivered exactly as transcribed. It never reaches a text
/// model, so it has no prompt to edit.
pub const PLAIN: &str = "plain";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltInAction {
    pub id: &'static str,
    pub name: &'static str,
    pub hint: &'static str,
    /// The shipped instructions. Empty for [`PLAIN`], which asks for nothing.
    pub prompt: &'static str,
}

/// What the user put in place of a shipped name or prompt. A field left `None`
/// still follows the default, so improving a default later still reaches
/// someone who only renamed the action, and a field left blank falls back the
/// same way rather than running the action on nothing.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionOverride {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
}

pub const BUILT_INS: &[BuiltInAction] = &[
    BuiltInAction {
        id: PLAIN,
        name: "Plain",
        hint: "Transcription only",
        prompt: "",
    },
    BuiltInAction {
        id: "clean",
        name: "Clean",
        hint: "Fix punctuation and obvious errors",
        prompt: "Correct punctuation, capitalization, spelling, and paragraph breaks. Preserve the speaker's language, wording, intent, names, numbers, and level of detail. Remove only obvious filler words. Return only the corrected text.",
    },
    BuiltInAction {
        id: "polish",
        name: "Polish",
        hint: "Rewrite for clarity",
        prompt: "Rewrite the transcript into clear, fluent prose in the speaker's language. Preserve every material fact, requirement, name, number, and the original intent. Do not add information. Return only the polished text.",
    },
    BuiltInAction {
        id: "summarize",
        name: "Summarize",
        hint: "Keep the essentials",
        prompt: "Summarize the transcript concisely in the speaker's language. Retain decisions, requirements, action items, names, numbers, and caveats. Use short paragraphs or bullets when helpful. Return only the summary.",
    },
    BuiltInAction {
        id: "prompt",
        name: "Prompt",
        hint: "Shape it into an AI prompt",
        prompt: "Convert the transcript into a precise, self-contained prompt for an AI assistant. Preserve all requirements, constraints, examples, and desired output. Remove conversational filler and resolve only unambiguous references. Return only the prompt.",
    },
    BuiltInAction {
        id: "email",
        name: "Email",
        hint: "Turn it into a ready-to-send email",
        prompt: "Write the transcript as an email in the speaker's language. Open with a fitting salutation and close with a sign-off. Keep every fact, request, name, number, date, and deadline, and add nothing that was not said. Use short paragraphs, and begin with a subject line when the transcript implies one. Return only the email.",
    },
];

pub fn find(id: &str) -> Option<&'static BuiltInAction> {
    BUILT_INS.iter().find(|action| action.id == id)
}

/// The instructions a built-in action runs with: the user's replacement where
/// there is one, the shipped text otherwise. An override that was emptied falls
/// back to the default rather than failing the recording that used it.
pub fn instructions(id: &str, overrides: &BTreeMap<String, ActionOverride>) -> Option<String> {
    let default = find(id)?.prompt;
    let replacement = overrides
        .get(id)
        .and_then(|value| value.prompt.as_deref())
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty());
    let resolved = replacement.unwrap_or(default).trim();
    (!resolved.is_empty()).then(|| resolved.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overrides(id: &str, value: ActionOverride) -> BTreeMap<String, ActionOverride> {
        BTreeMap::from([(id.to_string(), value)])
    }

    #[test]
    fn every_built_in_but_plain_ships_instructions() {
        for action in BUILT_INS {
            assert!(!action.name.is_empty());
            assert_eq!(
                action.prompt.is_empty(),
                action.id == PLAIN,
                "{} should {}have a prompt",
                action.id,
                if action.id == PLAIN { "not " } else { "" }
            );
        }
        assert!(find("email").is_some(), "0.4.2 ships an email prompt");
    }

    #[test]
    fn ids_are_unique() {
        let mut seen = Vec::new();
        for action in BUILT_INS {
            assert!(!seen.contains(&action.id), "duplicate action {}", action.id);
            seen.push(action.id);
        }
    }

    #[test]
    fn plain_has_no_instructions_to_run() {
        assert_eq!(instructions(PLAIN, &BTreeMap::new()), None);
    }

    #[test]
    fn an_unknown_action_has_none() {
        assert_eq!(instructions("nonsense", &BTreeMap::new()), None);
    }

    #[test]
    fn the_default_applies_until_it_is_replaced() {
        let default = instructions("clean", &BTreeMap::new()).unwrap();
        assert_eq!(default, find("clean").unwrap().prompt);

        let renamed = overrides(
            "clean",
            ActionOverride {
                name: Some("Tidy".into()),
                prompt: None,
            },
        );
        assert_eq!(
            instructions("clean", &renamed).unwrap(),
            default,
            "renaming an action must not freeze its instructions"
        );
    }

    #[test]
    fn a_replacement_wins_and_an_emptied_one_falls_back() {
        let replaced = overrides(
            "polish",
            ActionOverride {
                name: None,
                prompt: Some("  Write it as haiku.  ".into()),
            },
        );
        assert_eq!(
            instructions("polish", &replaced).unwrap(),
            "Write it as haiku."
        );

        let emptied = overrides(
            "polish",
            ActionOverride {
                name: None,
                prompt: Some("   \n ".into()),
            },
        );
        assert_eq!(
            instructions("polish", &emptied).unwrap(),
            find("polish").unwrap().prompt,
            "a blank replacement must not cost the recording that used it"
        );
    }
}
