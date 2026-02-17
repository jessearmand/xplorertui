use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    User(String),
    Search(String),
    Open(String),
    Home,
    Mentions,
    Bookmarks,
    Help,
    Auth,
    Quit,
}

pub fn parse_command(input: &str) -> Option<Command> {
    let input = input.strip_prefix(':').unwrap_or(input).trim();

    if input.is_empty() {
        return None;
    }

    let (cmd, args) = match input.split_once(char::is_whitespace) {
        Some((cmd, args)) => (cmd, args.trim()),
        None => (input, ""),
    };

    match cmd {
        "user" if !args.is_empty() => Some(Command::User(strip_at(args).to_owned())),
        "search" if !args.is_empty() => Some(Command::Search(args.to_owned())),
        "open" if !args.is_empty() => Some(Command::Open(args.to_owned())),
        "home" => Some(Command::Home),
        "mentions" | "m" => Some(Command::Mentions),
        "bookmarks" | "b" => Some(Command::Bookmarks),
        "help" | "h" => Some(Command::Help),
        "auth" | "login" => Some(Command::Auth),
        "quit" | "q" => Some(Command::Quit),
        _ => None,
    }
}

pub fn parse_tweet_url(input: &str) -> Option<String> {
    let trimmed = input.trim();

    // Raw numeric ID
    if trimmed.chars().all(|c| c.is_ascii_digit()) && !trimmed.is_empty() {
        return Some(trimmed.to_owned());
    }

    let url = Url::parse(trimmed).ok()?;

    let host = url.host_str()?;
    if host != "x.com" && host != "www.x.com" {
        return None;
    }

    // Path: /<user>/status/<id>
    let segments: Vec<&str> = url.path_segments()?.collect();

    if segments.len() >= 3 && segments[1] == "status" {
        let id = segments[2];
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
            return Some(id.to_owned());
        }
    }

    None
}

pub fn strip_at(username: &str) -> &str {
    username.strip_prefix('@').unwrap_or(username)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_command_user() {
        assert_eq!(
            parse_command(":user @alice"),
            Some(Command::User("alice".into()))
        );
        assert_eq!(parse_command("user bob"), Some(Command::User("bob".into())));
    }

    #[test]
    fn test_parse_command_search() {
        assert_eq!(
            parse_command(":search rust lang"),
            Some(Command::Search("rust lang".into()))
        );
    }

    #[test]
    fn test_parse_command_aliases() {
        assert_eq!(parse_command(":q"), Some(Command::Quit));
        assert_eq!(parse_command(":h"), Some(Command::Help));
        assert_eq!(parse_command(":b"), Some(Command::Bookmarks));
        assert_eq!(parse_command(":m"), Some(Command::Mentions));
        assert_eq!(parse_command(":auth"), Some(Command::Auth));
        assert_eq!(parse_command(":login"), Some(Command::Auth));
    }

    #[test]
    fn test_parse_command_empty() {
        assert_eq!(parse_command(""), None);
        assert_eq!(parse_command(":"), None);
    }

    #[test]
    fn test_parse_tweet_url_x() {
        assert_eq!(
            parse_tweet_url("https://x.com/user/status/123456"),
            Some("123456".into())
        );
    }

    #[test]
    fn test_parse_tweet_url_www_x() {
        assert_eq!(
            parse_tweet_url("https://www.x.com/user/status/789"),
            Some("789".into())
        );
    }

    #[test]
    fn test_parse_tweet_url_raw_id() {
        assert_eq!(parse_tweet_url("123456789"), Some("123456789".into()));
    }

    #[test]
    fn test_parse_tweet_url_invalid() {
        assert_eq!(parse_tweet_url("https://example.com/status/123"), None);
        assert_eq!(parse_tweet_url("not a url at all"), None);
    }

    #[test]
    fn test_strip_at() {
        assert_eq!(strip_at("@alice"), "alice");
        assert_eq!(strip_at("bob"), "bob");
    }
}
