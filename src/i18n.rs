use std::collections::HashMap;
use std::fs;

#[derive(Clone)]
pub struct Translator {
    dict: HashMap<String, String>,
}

impl Translator {
    pub fn new(lang: &str) -> Self {
        let mut dict = HashMap::new();
        if lang != "en" {
            if let Ok(data) = fs::read_to_string("languagepack.json") {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data) {
                    if let Some(target_lang) = parsed.get(lang) {
                        if let Some(obj) = target_lang.as_object() {
                            for (k, v) in obj {
                                if let Some(vs) = v.as_str() {
                                    dict.insert(k.clone(), vs.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        Translator { dict }
    }

    pub fn t(&self, key: &str, default_en: &str, args: &[(&str, &str)]) -> String {
        let mut text = self.dict.get(key).cloned().unwrap_or_else(|| default_en.to_string());
        for (k, v) in args {
            text = text.replace(&format!("{{{}}}", k), *v);
        }
        text
    }
}
