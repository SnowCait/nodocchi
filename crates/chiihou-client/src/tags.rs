pub fn has_tag_value(tags: &[Vec<String>], tag_name: &str, value: &str) -> bool {
    tags.iter().any(|tag| {
        tag.first().is_some_and(|name| name == tag_name) && tag.get(1).is_some_and(|v| v == value)
    })
}

pub fn root_channel_id(tags: &[Vec<String>]) -> Option<&str> {
    tags.iter().find_map(|tag| {
        let name = tag.first()?;
        let value = tag.get(1)?;
        let marker = tag.get(3)?;
        (name == "e" && marker == "root").then_some(value.as_str())
    })
}

pub fn build_reply_tags(
    request_event_id: &str,
    channel_id: &str,
    ai_pubkey_hex: &str,
    server_pubkey_hex: &str,
) -> Vec<Vec<String>> {
    vec![
        vec![
            "e".to_string(),
            channel_id.to_string(),
            String::new(),
            "root".to_string(),
        ],
        vec![
            "e".to_string(),
            request_event_id.to_string(),
            String::new(),
            "reply".to_string(),
            server_pubkey_hex.to_string(),
        ],
        vec!["p".to_string(), ai_pubkey_hex.to_string()],
        vec!["p".to_string(), server_pubkey_hex.to_string()],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn has_tag_value_matches_name_and_value() {
        let tags = vec![tag(&["e", "event1", "", "root"]), tag(&["p", "pubkey1"])];
        assert!(has_tag_value(&tags, "e", "event1"));
        assert!(has_tag_value(&tags, "p", "pubkey1"));
        assert!(!has_tag_value(&tags, "e", "pubkey1"));
        assert!(!has_tag_value(&tags, "p", "event1"));
        assert!(!has_tag_value(&tags, "t", "event1"));
    }

    #[test]
    fn has_tag_value_ignores_incomplete_tags() {
        let tags = vec![tag(&[]), tag(&["e"])];
        assert!(!has_tag_value(&tags, "e", "event1"));
    }

    #[test]
    fn root_channel_id_finds_root_e_tag() {
        let tags = vec![
            tag(&["p", "pubkey1"]),
            tag(&["e", "reply1", "", "reply"]),
            tag(&["e", "channel1", "", "root"]),
        ];
        assert_eq!(root_channel_id(&tags), Some("channel1"));
    }

    #[test]
    fn root_channel_id_is_none_without_root_marker() {
        let tags = vec![
            tag(&["e", "event1"]),
            tag(&["e", "event2", "", "reply"]),
            tag(&["p", "channel1", "", "root"]),
        ];
        assert_eq!(root_channel_id(&tags), None);
        assert_eq!(root_channel_id(&[]), None);
    }

    #[test]
    fn builds_reply_tags_in_nip28_reply_format() {
        assert_eq!(
            build_reply_tags("server_event", "channel", "ai_pubkey", "server_pubkey"),
            vec![
                tag(&["e", "channel", "", "root"]),
                tag(&["e", "server_event", "", "reply", "server_pubkey"]),
                tag(&["p", "ai_pubkey"]),
                tag(&["p", "server_pubkey"]),
            ]
        );
    }
}
