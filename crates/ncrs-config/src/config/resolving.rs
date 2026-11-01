use super::*;

/// Trimmed, without empty or repeated entries and without those `is_valid`
/// refuses.
pub fn own_entries(entries: &[String], is_valid: impl Fn(&str) -> bool) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for entry in entries.iter().map(|entry| entry.trim()) {
        if !entry.is_empty() && is_valid(entry) && !kept.iter().any(|known| known == entry) {
            kept.push(entry.to_owned());
        }
    }
    kept
}

/// Text rules only, no file system access: a dead mount must not block the
/// start or cost the user the rest of the config.
pub fn resolve_filing(section: &FilingSection, home: Option<&Path>) -> Result<Filing, FilingError> {
    let mut filing = Filing {
        enabled: section.enabled,
        locations: Vec::new(),
        load: resolve_load(section.load.as_ref())?,
        ai: resolve_ai(section.ai.as_ref())?,
        default_pattern: section
            .default_pattern
            .as_deref()
            .map(str::trim)
            .filter(|pattern| !pattern.is_empty())
            .map(ncrs_intake::upgraded_pattern)
            .filter(|pattern| pattern_is_valid(pattern)),
        own_names: OwnDetail::Names.entries_in(section),
        own_ibans: OwnDetail::Ibans.entries_in(section),
        own_tax_ids: OwnDetail::TaxIds.entries_in(section),
    };
    let Some(listed) = &section.locations else {
        return Ok(filing);
    };
    let entries = listed.get_ref();
    for entry in entries {
        let path_error = |reason: String| FilingError {
            span: entry.path.span(),
            reason,
        };
        let path = expand_location_path(entry.path.get_ref(), home).map_err(path_error)?;
        filing.locations.push(FilingLocation {
            path,
            mode: LocationMode::from(*entry.mode.get_ref()),
            images: entry.images,
            is_target: entry.target.as_ref().is_some_and(|flag| *flag.get_ref()),
        });
    }
    check_filing_rules(&filing.locations).map_err(|violation| {
        let target_flag_span = |index: usize| {
            entries
                .get(index)
                .and_then(|entry| entry.target.as_ref())
                .map_or_else(|| listed.span(), Spanned::span)
        };
        let path_of = |index: usize| {
            filing
                .locations
                .get(index)
                .map(|location| location.path.display().to_string())
                .unwrap_or_default()
        };
        match violation {
            FilingRuleViolation::InboxAsTarget { index } => FilingError {
                span: target_flag_span(index),
                reason: "an inbox cannot be the target, set `target = true` on an archive"
                    .to_owned(),
            },
            FilingRuleViolation::SecondTarget { index } => FilingError {
                span: target_flag_span(index),
                reason: "only one location can have `target = true`".to_owned(),
            },
            FilingRuleViolation::NoTarget => FilingError {
                span: listed.span(),
                reason: "no location has `target = true`, one archive must be the target"
                    .to_owned(),
            },
            FilingRuleViolation::Overlap { earlier, later } => FilingError {
                span: entries
                    .get(later)
                    .map_or_else(|| listed.span(), |entry| entry.path.span()),
                reason: format!(
                    "{} and {} overlap, a location cannot be the same folder as another or lie inside it",
                    path_of(earlier),
                    path_of(later)
                ),
            },
        }
    })?;
    Ok(filing)
}

pub fn resolve_ai(section: Option<&Spanned<AiSection>>) -> Result<AiSetting, FilingError> {
    let Some(spanned) = section else {
        return Ok(AiSetting::Off);
    };
    let ai = spanned.get_ref();
    let provider = ai
        .provider
        .as_ref()
        .map_or_else(AiProviderName::default, |named| *named.get_ref());
    let only_for = |key: &Option<Spanned<String>>, name: &str, wanted: AiProviderName| match key {
        Some(given) if provider != wanted => Err(FilingError {
            span: given.span(),
            reason: format!(
                "{name} is only for provider = \"{}\"",
                provider_text(wanted)
            ),
        }),
        _ => Ok(()),
    };
    let threads = match &ai.threads {
        Some(given) if provider == AiProviderName::Http => {
            return Err(FilingError {
                span: given.span(),
                reason: "threads is only for provider = \"local\"".to_owned(),
            });
        }
        Some(given) => Some(
            u32::try_from(*given.get_ref())
                .ok()
                .and_then(NonZeroU32::new)
                .ok_or_else(|| FilingError {
                    span: given.span(),
                    reason: "threads must be a number from 1 up".to_owned(),
                })?,
        ),
        None => None,
    };
    only_for(&ai.endpoint, "endpoint", AiProviderName::Http)?;
    only_for(&ai.model, "model", AiProviderName::Http)?;
    if !ai.enabled {
        return Ok(AiSetting::Off);
    }
    match provider {
        AiProviderName::Local => Ok(AiSetting::Local { threads }),
        AiProviderName::Http => {
            let missing = |key: &str| FilingError {
                span: spanned.span(),
                reason: format!("provider = \"http\" needs {key}"),
            };
            let endpoint = ai.endpoint.as_ref().ok_or_else(|| missing("endpoint"))?;
            let model = ai.model.as_ref().ok_or_else(|| missing("model"))?;
            check_ai_endpoint(endpoint.get_ref()).map_err(|problem| FilingError {
                span: endpoint.span(),
                reason: problem.reason(),
            })?;
            Ok(AiSetting::Http {
                endpoint: endpoint.get_ref().clone(),
                model: model.get_ref().clone(),
            })
        }
    }
}

/// Why `endpoint` cannot be the address of `provider = "http"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiEndpointProblem {
    /// Not an address the client takes; the text says why.
    Unusable(String),
    /// Not on this machine.
    NotLocal,
}

impl AiEndpointProblem {
    /// The reason in a config error, which is not translated.
    fn reason(&self) -> String {
        match self {
            Self::Unusable(problem) => format!("endpoint is not usable: {problem}"),
            Self::NotLocal => {
                "endpoint must be on this machine (localhost, 127.0.0.1 or ::1) for now".to_owned()
            }
        }
    }
}

/// The checks `load` makes on `endpoint`, for the settings tab to make them
/// before anything is written.
pub fn check_ai_endpoint(endpoint: &str) -> Result<(), AiEndpointProblem> {
    let unusable =
        |problem: ncrs_ai::EndpointProblem| AiEndpointProblem::Unusable(problem.to_string());
    ncrs_ai::check_endpoint(endpoint).map_err(unusable)?;
    if ncrs_ai::endpoint_is_local(endpoint).map_err(unusable)? {
        Ok(())
    } else {
        Err(AiEndpointProblem::NotLocal)
    }
}

pub const fn provider_text(provider: AiProviderName) -> &'static str {
    match provider {
        AiProviderName::Local => "local",
        AiProviderName::Http => "http",
    }
}

pub fn resolve_load(value: Option<&Spanned<LoadValue>>) -> Result<Load, FilingError> {
    let Some(spanned) = value else {
        return Ok(Load::default());
    };
    let resolved = match spanned.get_ref() {
        LoadValue::Name(name) => match name.as_str() {
            "gentle" => Some(Load::Gentle),
            "balanced" => Some(Load::Balanced),
            "full" => Some(Load::Full),
            _ => None,
        },
        LoadValue::Count(count) => usize::try_from(*count)
            .ok()
            .and_then(NonZeroUsize::new)
            .map(Load::Workers),
    };
    resolved.ok_or_else(|| FilingError {
        span: spanned.span(),
        reason: r#"load must be "gentle", "balanced", "full" or a number of workers from 1 up"#
            .to_owned(),
    })
}

/// `~` and `~/rest` become the home directory; everything else must already
/// be absolute. A `..` component would make the overlap check wrong.
pub fn expand_location_path(text: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let expanded = match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(is_separator) => {
            let known_home = home.ok_or("no home directory to expand `~`")?;
            known_home.join(rest.trim_start_matches(is_separator))
        }
        _ => PathBuf::from(text),
    };
    if !expanded.is_absolute() {
        return Err(format!(
            "the path \"{text}\" must be absolute or start with `~/`"
        ));
    }
    if expanded
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(format!("the path \"{text}\" must not contain `..`"));
    }
    Ok(expanded)
}

/// 1-based line of the byte at `offset`.
pub fn line_at(source: &[u8], offset: usize) -> Option<usize> {
    let before = source.get(..offset)?;
    Some(before.iter().filter(|&&byte| byte == b'\n').count() + 1)
}
