use super::{Detection, LANGUAGES};

#[derive(Debug, PartialEq)]
pub(crate) enum LanguageChoice {
    Ready(String),
    NeedsChoice(String),
    Unsupported(String),
}

pub(crate) fn choose_language(manual: Option<&str>, detections: &[Detection]) -> LanguageChoice {
    if let Some(code) = manual {
        return if LANGUAGES.iter().any(|(supported, _)| code == *supported) {
            LanguageChoice::Ready(code.into())
        } else {
            LanguageChoice::Unsupported(code.into())
        };
    }
    if detections.is_empty()
        || detections.iter().any(|d| {
            !(0.8..=1.).contains(&d.score)
                || d.alternatives
                    .first()
                    .is_some_and(|(_, second)| !second.is_finite() || d.score - second < 0.2)
        })
    {
        return LanguageChoice::NeedsChoice(
            "Language detection is uncertain. Choose the spoken language to continue.".into(),
        );
    }
    // Both tested detectors confused these even with high scores. Spoken
    // Hindustani alone cannot reliably determine the intended written script.
    if detections
        .iter()
        .any(|d| matches!(d.language.as_str(), "hi" | "ur"))
    {
        return LanguageChoice::NeedsChoice("Hindi or Urdu speech detected. Choose Hindi (Devanagari) or Urdu (Arabic script) before transcription.".into());
    }
    let first = &detections[0].language;
    if detections.iter().any(|d| &d.language != first) {
        return LanguageChoice::NeedsChoice("Different speech sections produced different languages. Choose the main language, or analyze separate clips.".into());
    }
    if LANGUAGES.iter().any(|(supported, _)| first == supported) {
        LanguageChoice::Ready(first.clone())
    } else {
        LanguageChoice::Unsupported(first.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn detection(language: &str, score: f64, second: &str, second_score: f64) -> Detection {
        Detection {
            start_s: 0.,
            end_s: 10.,
            language: language.into(),
            score,
            alternatives: vec![(second.into(), second_score)],
        }
    }

    #[test]
    fn hindi_urdu_confidence_does_not_select_the_wrong_script() {
        let candidates = [detection("hi", 0.946788, "ur", 0.047549)];
        assert!(matches!(
            choose_language(None, &candidates),
            LanguageChoice::NeedsChoice(_)
        ));
        assert_eq!(
            choose_language(Some("ur"), &candidates),
            LanguageChoice::Ready("ur".into())
        );
        assert!(matches!(
            choose_language(None, &[detection("ur", 0.999, "hi", 0.001)]),
            LanguageChoice::NeedsChoice(_)
        ));
    }

    #[test]
    fn uncertain_and_out_of_scope_languages_do_not_trigger_forced_model_selection() {
        assert!(matches!(
            choose_language(None, &[detection("jw", 0.633329, "id", 0.364386)]),
            LanguageChoice::NeedsChoice(_)
        ));
        assert_eq!(
            choose_language(None, &[detection("ja", 0.99, "zh", 0.01)]),
            LanguageChoice::Unsupported("ja".into())
        );
        assert_eq!(
            choose_language(None, &[detection("fr", 0.99, "en", 0.01)]),
            LanguageChoice::Ready("fr".into())
        );
    }

    #[test]
    fn disagreeing_sections_need_review_instead_of_a_majority_language_guess() {
        assert!(matches!(
            choose_language(
                None,
                &[
                    detection("fr", 0.99, "en", 0.01),
                    detection("en", 0.99, "fr", 0.01)
                ]
            ),
            LanguageChoice::NeedsChoice(_)
        ));
        assert!(matches!(
            choose_language(None, &[]),
            LanguageChoice::NeedsChoice(_)
        ));
        assert!(matches!(
            choose_language(None, &[detection("en", f64::NAN, "fr", 0.)]),
            LanguageChoice::NeedsChoice(_)
        ));
    }
}
