use super::dfa::{Cylon, Rule};
use futures_util::{
    io::{AsyncBufRead, AsyncRead, BufReader, Result},
    AsyncBufReadExt,
};
use serde_derive::{Deserialize, Serialize};
const UA_PREFIX: &str = "user-agent:";
const DELAY_PREFIX: &str = "crawl-delay:";
const ALLOW_PREFIX: &str = "allow:";
const DISALLOW_PREFIX: &str = "disallow:";

#[derive(Debug, PartialEq, Clone)]
enum ParsedRule {
    Allow(String),
    Disallow(String),
    Delay(u64),
}

impl<'a> Into<Rule<'a>> for &'a ParsedRule {
    fn into(self) -> Rule<'a> {
        match self {
            ParsedRule::Allow(path) => Rule::Allow(&path[..]),
            ParsedRule::Disallow(path) => Rule::Disallow(&path[..]),
            ParsedRule::Delay(delay) => Rule.Delay(delay),
        }
    }
}

#[derive(Debug, PartialEq)]
enum ParsedLine {
    UserAgent(String),
    Rule(ParsedRule),
    Nothing,
}

/// A compiler takes an input robots.txt file and outputs a compiled Cylon,
/// which can be used to efficiently match a large number of paths against
/// the robots.txt file.
#[derive(Debug, Serialize, Deserialize)]
pub struct Compiler {
    user_agent: String,
}

impl Compiler {
    /// Build a new compiler that parses rules for the given user agent from
    /// a robots.txt file.
    pub fn new(user_agent: &str) -> Self {
        Self {
            user_agent: user_agent.to_lowercase(),
        }
    }

    /// Parse an input robots.txt file into a Cylon that can recognize
    /// whether or not a path matches the rules for the Parser's user agent.
    pub async fn compile<R: AsyncRead + Unpin>(&self, file: R) -> Result<Cylon> {
        let reader = BufReader::new(file);
        let mut agent = String::new();
        let mut rules: Vec<ParsedRule> = vec![];
        let mut group_reader = GroupReader::new(reader);

        // find the most specific matching group in the robots file
        while let Some(agents) = group_reader.next_header().await? {
            let matching_agent = agents.iter().find(|a| {
                let matches = &a[..] == "*" || self.user_agent.contains(*a);
                let more_specific = a.len() > agent.len();
                matches && more_specific
            });

            if let Some(matching_agent) = matching_agent {
                agent = matching_agent.clone();
                rules = group_reader.next_rules().await?;
            }
        }

        let rules = rules.iter().map(|r| r.into()).collect();
        Ok(Cylon::compile(rules))
    }
}

struct GroupReader<R: AsyncBufRead + Unpin> {
    parsing_agents: bool,
    agents: Vec<String>,
    rules: Vec<ParsedRule>,
    reader: R,
}

impl<R: AsyncBufRead + Unpin> GroupReader<R> {
    fn new(reader: R) -> Self {
        Self {
            parsing_agents: true,
            agents: vec![],
            rules: vec![],
            reader,
        }
    }

    /// Scan forward until the next group header defined by one or more
    /// user agent lines. This lets us optimize the lines we need to copy
    /// so we can skip over groups that don't match the desired user agent.
    async fn next_header(&mut self) -> Result<Option<Vec<String>>> {
        let mut buf = String::new();
        while self.reader.read_line(&mut buf).await? != 0 {
            let parsed_line = parse_line(buf.clone());

            match parsed_line {
                ParsedLine::UserAgent(ua) if self.parsing_agents => {
                    self.agents.push(ua);
                }
                ParsedLine::UserAgent(ua) => {
                    self.agents = vec![ua];
                    self.rules = vec![];
                    self.parsing_agents = true;
                }
                ParsedLine::Rule(rule) if self.parsing_agents => {
                    // Preserve the rule in case we need it in next_rules().
                    self.rules.push(rule);
                    self.parsing_agents = false;
                    break;
                }
                // Skip over lines until we get to the next user agent.
                ParsedLine::Rule(..) => (),
                ParsedLine::Nothing => (),
            }

            buf.clear();
        }

        let agents = self.agents.clone();
        self.agents = vec![];

        if agents.is_empty() {
            return Ok(None);
        }

        Ok(Some(agents))
    }

    async fn next_rules(&mut self) -> Result<Vec<ParsedRule>> {
        let mut buf = String::new();
        while self.reader.read_line(&mut buf).await? != 0 {
            let parsed_line = parse_line(buf.clone());

            match parsed_line {
                ParsedLine::Rule(rule) => {
                    self.rules.push(rule);
                    self.parsing_agents = false;
                }
                ParsedLine::UserAgent(ua) if !self.parsing_agents => {
                    // Preserve the agent in case we need it in next_agents().
                    self.agents.push(ua);
                    self.parsing_agents = true;
                    break;
                }
                // Skip over lines until we get to the next rule.
                ParsedLine::UserAgent(..) => (),
                ParsedLine::Nothing => (),
            }

            buf.clear();
        }

        let rules = self.rules.clone();
        self.rules = vec![];
        Ok(rules)
    }
}

fn parse_line(line: String) -> ParsedLine {
    let line = strip_comments(&line[..]).trim();

    // This tries to parse lines roughly in order of most frequent kind to
    // least frequent kind in order to minimize CPU cycles on average.
    parse_disallow(line)
        .map(|s| ParsedLine::Rule(ParsedRule::Disallow(s.into())))
        .or_else(|| parse_user_agent(line).map(|s| ParsedLine::UserAgent(s.to_lowercase())))
        .or_else(|| parse_allow(line).map(|s| ParsedLine::Rule(ParsedRule::Allow(s.into()))))
        .or_else(|| parse_delay(line).map(|s| ParsedLine::Rule(ParsedRule::Delay(s.into()))))
        .unwrap_or(ParsedLine::Nothing)
}

fn strip_comments(line: &str) -> &str {
    if let Some(before) = line.split('#').next() {
        return before;
    }
    return line;
}

fn parse_user_agent(line: &str) -> Option<&str> {
    if line.len() < UA_PREFIX.len() {
        return None;
    }
    let prefix = &line[..UA_PREFIX.len()].to_ascii_lowercase();
    let suffix = &line[UA_PREFIX.len()..];

    if prefix == UA_PREFIX {
        Some(suffix.trim())
    } else {
        None
    }
}

fn parse_delay(line: &str) -> Option<u64> {
    if line.len() < DELAY_PREFIX.len() {
        return None;
    }

    let prefix = &line[..DELAY_PREFIX.len()].to_ascii_lowercase();
    let suffix = &line[DELAY_PREFIX.len()..];
    if prefix == DELAY_PREFIX {
        Some(suffix.trim())
    } else {
        None
    }
}

fn parse_allow(line: &str) -> Option<&str> {
    if line.len() < ALLOW_PREFIX.len() {
        return None;
    }
    let prefix = &line[..ALLOW_PREFIX.len()].to_ascii_lowercase();
    let suffix = &line[ALLOW_PREFIX.len()..];

    if prefix == ALLOW_PREFIX {
        Some(suffix.trim())
    } else {
        None
    }
}

fn parse_disallow(line: &str) -> Option<&str> {
    if line.len() < DISALLOW_PREFIX.len() {
        return None;
    }
    let prefix = &line[..DISALLOW_PREFIX.len()].to_ascii_lowercase();
    let suffix = &line[DISALLOW_PREFIX.len()..];

    if prefix == DISALLOW_PREFIX {
        Some(suffix.trim())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_allow() {
        let test_cases = vec![
            ("Allow: /", "/"),
            ("allow: /   #  Root with comment", "/"),
            ("ALLOW: /abc/def  ", "/abc/def"),
            ("Allow:   /abc/def  ", "/abc/def"),
            ("  Allow: /*/foo", "/*/foo"),
        ];

        for (i, o) in test_cases {
            assert_eq!(
                parse_line(i.into()),
                ParsedLine::Rule(ParsedRule::Allow(o.into()))
            );
        }
    }

    #[test]
    fn test_parse_disallow() {
        let test_cases = vec![
            ("Disallow: /", "/"),
            ("disallow: /   #  Root with comment", "/"),
            ("DISALLOW: /abc/def  ", "/abc/def"),
            ("Disallow:   /abc/def  ", "/abc/def"),
            ("  Disallow: /*/foo", "/*/foo"),
        ];

        for (i, o) in test_cases {
            assert_eq!(
                parse_line(i.into()),
                ParsedLine::Rule(ParsedRule::Disallow(o.into()))
            );
        }
    }

    #[test]
    fn test_parse_user_agent() {
        let test_cases = vec![
            ("User-agent: *", "*"),
            ("user-agent: ImABot   #  User agent with comment", "imabot"),
            ("  USER-AGENT:   ImABot  ", "imabot"),
        ];

        for (i, o) in test_cases {
            assert_eq!(parse_line(i.into()), ParsedLine::UserAgent(o.into()));
        }
    }

    #[test]
    fn test_parse_nothing() {
        let test_cases = vec![
            "Useragent: *",
            "# Comment",
            "",
            "    ",
            "\t",
            "alow: /",
            "disalow: /",
        ];

        for i in test_cases {
            assert_eq!(parse_line(i.into()), ParsedLine::Nothing);
        }
    }

    #[test]
    fn test_end_to_end() {
        tokio_test::block_on(async {
            let example_robots = r#"
            User-agent: jones-bot
            Disallow: /

            User-agent: jones
            User-agent: foobar
            Allow: /

            User-agent: *
            Disallow: /
            "#
            .as_bytes();

            let parser = Compiler::new("foobar");
            let foobar_machine = parser.compile(example_robots).await.unwrap();

            let parser = Compiler::new("jones-bot");
            let jonesbot_machine = parser.compile(example_robots).await.unwrap();

            let parser = Compiler::new("imabot");
            let imabot_machine = parser.compile(example_robots).await.unwrap();

            let parser = Compiler::new("abc");
            let abc_machine = parser.compile(example_robots).await.unwrap();

            assert_eq!(true, foobar_machine.allow("/index.html"));
            assert_eq!(false, jonesbot_machine.allow("/index.html"));
            assert_eq!(false, imabot_machine.allow("/index.html"));
            assert_eq!(false, abc_machine.allow("/index.html"));
        });
    }

    #[test]
    fn test_invalid_1() {
        tokio_test::block_on(async {
            let example_robots = r#"
            # Instead of treating this as an error, we'll just consider
            # this behavior undefined.
            Allow: /

            User-agent: jones
            User-agent: foobar
            Disallow: /
            "#
            .as_bytes();

            let parser = Compiler::new("foobar");
            let foobar_machine = parser.compile(example_robots).await.unwrap();

            let parser = Compiler::new("imabot");
            let imabot_machine = parser.compile(example_robots).await.unwrap();

            // Everything is allowed because next_header() returns None
            assert_eq!(true, foobar_machine.allow("/index.html"));
            assert_eq!(true, imabot_machine.allow("/index.html"));
        });
    }

    #[test]
    fn test_invalid_2() {
        tokio_test::block_on(async {
            let example_robots = r#"
            User-agent: jones
            User-agent: foobar
            Disallow: /

            # Instead of treating this as an error, we consider this
            # behavior undefined.
            User-agent: imabot
            "#
            .as_bytes();

            let parser = Compiler::new("foobar");
            let foobar_machine = parser.compile(example_robots).await.unwrap();

            let parser = Compiler::new("imabot");
            let imabot_machine = parser.compile(example_robots).await.unwrap();

            assert_eq!(false, foobar_machine.allow("/index.html"));
            assert_eq!(true, imabot_machine.allow("/index.html"));
        });
    }
}
