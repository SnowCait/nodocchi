use crate::protocol::ChiihouPai;

pub fn build_sutehai_reply_content(server_npub: &str, pai: ChiihouPai) -> String {
    format!("nostr:{server_npub} sutehai? sutehai {pai}")
}

pub fn build_sutehai_tsumo_reply_content(server_npub: &str) -> String {
    format!("nostr:{server_npub} sutehai? tsumo")
}

pub fn build_sutehai_richi_reply_content(server_npub: &str, pai: ChiihouPai) -> String {
    format!("nostr:{server_npub} sutehai? richi {pai}")
}

pub fn build_naku_no_reply_content(server_npub: &str) -> String {
    format!("nostr:{server_npub} naku? no")
}

pub fn build_naku_ron_reply_content(server_npub: &str) -> String {
    format!("nostr:{server_npub} naku? ron")
}

pub fn build_naku_pon_reply_content(server_npub: &str) -> String {
    format!("nostr:{server_npub} naku? pon")
}

pub fn build_naku_kan_reply_content(server_npub: &str) -> String {
    format!("nostr:{server_npub} naku? kan")
}

pub fn build_naku_chi_reply_content(server_npub: &str, consumed: [ChiihouPai; 2]) -> String {
    let [first, second] = consumed;
    format!("nostr:{server_npub} naku? chi {first} {second}")
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
    fn builds_sutehai_richi_reply_content_with_pai() {
        assert_eq!(
            build_sutehai_richi_reply_content("npub1server", "5p".parse().unwrap()),
            "nostr:npub1server sutehai? richi 5p"
        );
        assert_eq!(
            build_sutehai_richi_reply_content("npub1server", "1z".parse().unwrap()),
            "nostr:npub1server sutehai? richi 1z"
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

    #[test]
    fn builds_naku_pon_reply_content() {
        assert_eq!(
            build_naku_pon_reply_content("npub1server"),
            "nostr:npub1server naku? pon"
        );
    }

    #[test]
    fn builds_naku_kan_reply_content() {
        assert_eq!(
            build_naku_kan_reply_content("npub1server"),
            "nostr:npub1server naku? kan"
        );
    }

    #[test]
    fn builds_naku_chi_reply_content_with_two_pais() {
        assert_eq!(
            build_naku_chi_reply_content(
                "npub1server",
                ["1m".parse().unwrap(), "3m".parse().unwrap()]
            ),
            "nostr:npub1server naku? chi 1m 3m"
        );
        assert_eq!(
            build_naku_chi_reply_content(
                "npub1server",
                ["7s".parse().unwrap(), "8s".parse().unwrap()]
            ),
            "nostr:npub1server naku? chi 7s 8s"
        );
    }
}
