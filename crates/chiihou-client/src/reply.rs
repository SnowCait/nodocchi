use crate::protocol::ChiihouPai;

pub fn build_sutehai_reply_content(server_npub: &str, pai: ChiihouPai) -> String {
    format!("nostr:{server_npub} sutehai? sutehai {pai}")
}

pub fn build_sutehai_tsumo_reply_content(server_npub: &str) -> String {
    format!("nostr:{server_npub} sutehai? tsumo")
}

pub fn build_naku_no_reply_content(server_npub: &str) -> String {
    format!("nostr:{server_npub} naku? no")
}

pub fn build_naku_ron_reply_content(server_npub: &str) -> String {
    format!("nostr:{server_npub} naku? ron")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_sutehai_reply_content() {
        assert_eq!(
            build_sutehai_reply_content("npub1server", "7z".parse().unwrap()),
            "nostr:npub1server sutehai? sutehai 7z"
        );
        assert_eq!(
            build_sutehai_reply_content("npub1server", "5m".parse().unwrap()),
            "nostr:npub1server sutehai? sutehai 5m"
        );
    }

    #[test]
    fn builds_sutehai_tsumo_reply_content_without_pai() {
        assert_eq!(
            build_sutehai_tsumo_reply_content("npub1server"),
            "nostr:npub1server sutehai? tsumo"
        );
    }

    #[test]
    fn builds_naku_no_reply_content() {
        assert_eq!(
            build_naku_no_reply_content("npub1server"),
            "nostr:npub1server naku? no"
        );
    }

    #[test]
    fn builds_naku_ron_reply_content() {
        assert_eq!(
            build_naku_ron_reply_content("npub1server"),
            "nostr:npub1server naku? ron"
        );
    }
}
