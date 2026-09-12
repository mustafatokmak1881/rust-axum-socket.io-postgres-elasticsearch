pub fn oauth_flow(state_hash: &str) -> String {
    format!("oauth:flow:{state_hash}")
}

pub fn user(user_id: &str) -> String {
    format!("user:{user_id}")
}

pub fn user_by_google(google_sub: &str) -> String {
    format!("user:google:{google_sub}")
}

pub fn session(token_hash: &str) -> String {
    format!("session:{token_hash}")
}

pub fn entitlements(user_id: &str) -> String {
    format!("entitlement:{user_id}")
}

pub fn lobby(lobby_id: &str) -> String {
    format!("lobby:{lobby_id}")
}

pub fn lobbies_index() -> &'static str {
    "lobbies:open"
}

pub fn match_meta(match_id: &str) -> String {
    format!("match:{match_id}:meta")
}

pub fn match_stream(match_id: &str) -> String {
    format!("match:{match_id}:stream")
}
