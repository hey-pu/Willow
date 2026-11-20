use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenUnit {
    #[default]
    Chinese,
    International,
}

impl TokenUnit {
    pub fn format(self, value: i64) -> String {
        let units: &[(f64, &str)] = match self {
            Self::Chinese => &[(1.0, ""), (1e4, "万"), (1e8, "亿")],
            Self::International => &[(1.0, ""), (1e3, "K"), (1e6, "M"), (1e9, "B")],
        };
        let value = value as f64;
        let mut index = 0;
        while index + 1 < units.len() && value.abs() >= units[index + 1].0 { index += 1; }
        if index + 1 < units.len()
            && ((value.abs() / units[index].0) * 100.0).round() / 100.0 >= units[index + 1].0 / units[index].0
        { index += 1; }
        let number = format!("{:.2}", value / units[index].0);
        format!("{}{}", number.trim_end_matches('0').trim_end_matches('.'), units[index].1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_shared_examples() {
        let cases: serde_json::Value = serde_json::from_str(include_str!("../../tests/token-units.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let value = case["value"].as_i64().unwrap();
            assert_eq!(TokenUnit::Chinese.format(value), case["chinese"].as_str().unwrap());
            assert_eq!(TokenUnit::International.format(value), case["international"].as_str().unwrap());
        }
    }
    #[test]
    fn rejects_unknown_units() {
        assert!(serde_json::from_str::<TokenUnit>("\"full\"").is_err());
    }
}
