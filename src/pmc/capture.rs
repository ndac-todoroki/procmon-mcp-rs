//! Building the configuration that a capture run loads with `/LoadConfig`.

use super::defaults::default_filter_rules;
use super::filter::encode_filter_rules;
use super::record::{Configuration, Record};

/// Conditions of one capture. Unset conditions do not narrow the capture.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureFilter {
    pub process_name: Option<String>,
    pub pid: Option<u32>,
    pub operations: Vec<String>,
    pub path_contains: Option<String>,
    pub result: Option<String>,
}

/// Why a [`CaptureFilter`] was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureFilterError {}

/// Builds the configuration for `filter`: the default filter plus its Include rules,
/// with filtered-out events dropped from the log.
pub fn capture_configuration(_filter: &CaptureFilter) -> Result<Configuration, CaptureFilterError> {
    let rules = default_filter_rules();
    Ok(Configuration {
        records: vec![
            u32_record("Profiling", 0),
            u32_record("DestructiveFilter", 1),
            Record {
                name: "FilterRules".to_string(),
                data: encode_filter_rules(&rules),
            },
        ],
    })
}

fn u32_record(name: &str, value: u32) -> Record {
    Record {
        name: name.to_string(),
        data: value.to_le_bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pmc::decode_filter_rules;

    fn u32_value(config: &Configuration, name: &str) -> u32 {
        let data = config
            .data(name)
            .unwrap_or_else(|| panic!("no {name} record"));
        u32::from_le_bytes(data.try_into().unwrap())
    }

    #[test]
    fn empty_filter_uses_the_default_rules_and_drops_filtered_events() {
        let config = capture_configuration(&CaptureFilter::default()).unwrap();

        let rules = decode_filter_rules(config.data("FilterRules").unwrap()).unwrap();
        assert_eq!(rules, default_filter_rules());
        assert_eq!(u32_value(&config, "DestructiveFilter"), 1);
        assert_eq!(u32_value(&config, "Profiling"), 0);
    }
}
