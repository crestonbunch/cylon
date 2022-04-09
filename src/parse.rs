use std::collections::BTreeMap;

use super::nfa::{Cylon, Rule};
use futures_util::{
    io::{AsyncBufRead, AsyncRead, BufReader, Result},
    AsyncBufReadExt,
};
use serde_derive::{Deserialize, Serialize};
const UA_PREFIX: &str = "user-agent:";
#[cfg(feature = "crawl-delay")]
const DELAY_PREFIX: &str = "crawl-delay:";
const ALLOW_PREFIX: &str = "allow:";
const DISALLOW_PREFIX: &str = "disallow:";

#[derive(Debug, PartialEq, Clone)]
enum ParsedRule {
    Allow(String),
    Disallow(String),
    #[cfg(feature = "crawl-delay")]
    Delay(String),
}

impl<'a> From<&'a ParsedRule> for Rule<'a> {
    fn from(rule: &ParsedRule) -> Rule<'_> {
        match rule {
            ParsedRule::Allow(path) => Rule::Allow(path.as_bytes()),
            ParsedRule::Disallow(path) => Rule::Disallow(path.as_bytes()),
            #[cfg(feature = "crawl-delay")]
            ParsedRule::Delay(delay) => Rule::Delay(delay.as_bytes()),
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

        let rules = Compiler::filter_dupes(&rules);
        Ok(Cylon::compile(rules))
    }

    fn filter_dupes(rules: &[ParsedRule]) -> Vec<Rule<'_>> {
        let mut dedupe = BTreeMap::new();
        for rule in rules {
            match rule {
                ParsedRule::Allow(inner) => {
                    dedupe.insert(inner.clone(), rule.into());
                }
                #[cfg(feature = "crawl-delay")]
                ParsedRule::Delay(inner) => {
                    dedupe.insert(inner.clone(), rule.into());
                }
                ParsedRule::Disallow(inner) => {
                    if !dedupe.contains_key(inner) {
                        dedupe.insert(inner.clone(), rule.into());
                    }
                }
            }
        }
        dedupe.into_values().collect()
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

    #[cfg(feature = "crawl-delay")]
    return parse_disallow(line)
        .map(|s| ParsedLine::Rule(ParsedRule::Disallow(s.into())))
        .or_else(|| parse_user_agent(line).map(|s| ParsedLine::UserAgent(s.to_lowercase())))
        .or_else(|| parse_allow(line).map(|s| ParsedLine::Rule(ParsedRule::Allow(s.into()))))
        .or_else(|| parse_delay(line).map(|s| ParsedLine::Rule(ParsedRule::Delay(s.into()))))
        .unwrap_or(ParsedLine::Nothing);

    #[cfg(not(feature = "crawl-delay"))]
    return parse_disallow(line)
        .map(|s| ParsedLine::Rule(ParsedRule::Disallow(s.into())))
        .or_else(|| parse_user_agent(line).map(|s| ParsedLine::UserAgent(s.to_lowercase())))
        .or_else(|| parse_allow(line).map(|s| ParsedLine::Rule(ParsedRule::Allow(s.into()))))
        .unwrap_or(ParsedLine::Nothing);
}

fn strip_comments(line: &str) -> &str {
    if let Some(before) = line.split('#').next() {
        before
    } else {
        line
    }
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

#[cfg(feature = "crawl-delay")]
fn parse_delay(line: &str) -> Option<&str> {
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
    #[cfg(feature = "crawl-delay")]
    fn test_crawl_delay() {
        tokio_test::block_on(async {
            let example_robots = r#"
            User-agent: jones-bot
            Disallow: /
            Crawl-Delay: 30

            User-agent: foobar
            Crawl-Delay: 60

            User-agent: googlebot
            Allow: /

            User-agent: barfoo
            Crawl-Delay: 60
            Crawl-Delay: 20
            "#
            .as_bytes();

            let parser = Compiler::new("foobar");
            let foobar_machine = parser.compile(example_robots).await.unwrap();

            let parser = Compiler::new("googlebot");
            let googlebot_machine = parser.compile(example_robots).await.unwrap();

            let parser = Compiler::new("barfoo");
            let barfoo_machine = parser.compile(example_robots).await.unwrap();

            let parser = Compiler::new("jones-bot");
            let jonesbot_machine = parser.compile(example_robots).await.unwrap();

            assert_eq!(Some(60), foobar_machine.delay());
            assert_eq!(Some(20), barfoo_machine.delay());
            assert_eq!(Some(30), jonesbot_machine.delay());
            assert_eq!(None, googlebot_machine.delay());
        });
    }

    #[test]
    fn test_end_to_end() {
        tokio_test::block_on(async {
            let example_robots = r#"
            User-agent: jones-bot
            Disallow: /

            User-agent: foo
            Allow: /
            Crawl-Delay: 20

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

    #[test]
    fn test_unicode_support() {
        tokio_test::block_on(async {
            // From: wikipedia.org/robots.txt
            let example_robots = r#"
            User-agent: test
            Disallow: /wiki/ויקיפדיה:רשימת_מועמדים_למחיקה/
            Disallow: /wiki/ויקיפדיה%3Aרשימת_מועמדים_למחיקה/
            Disallow: /wiki/%D7%95%D7%99%D7%A7%D7%99%D7%A4%D7%93%D7%99%D7%94:%D7%A8%D7%A9%D7%99%D7%9E%D7%AA_%D7%9E%D7%95%D7%A2%D7%9E%D7%93%D7%99%D7%9D_%D7%9C%D7%9E%D7%97%D7%99%D7%A7%D7%94/
            Disallow: /wiki/%D7%95%D7%99%D7%A7%D7%99%D7%A4%D7%93%D7%99%D7%94%3A%D7%A8%D7%A9%D7%99%D7%9E%D7%AA_%D7%9E%D7%95%D7%A2%D7%9E%D7%93%D7%99%D7%9D_%D7%9C%D7%9E%D7%97%D7%99%D7%A7%D7%94/
            Disallow: /wiki/ויקיפדיה:ערכים_לא_קיימים_ומוגנים
            Disallow: /wiki/ויקיפדיה%3Aערכים_לא_קיימים_ומוגנים
            Disallow: /wiki/%D7%95%D7%99%D7%A7%D7%99%D7%A4%D7%93%D7%99%D7%94:%D7%A2%D7%A8%D7%9B%D7%99%D7%9D_%D7%9C%D7%90_%D7%A7%D7%99%D7%99%D7%9E%D7%99%D7%9D_%D7%95%D7%9E%D7%95%D7%92%D7%A0%D7%99%D7%9D
            Disallow: /wiki/%D7%95%D7%99%D7%A7%D7%99%D7%A4%D7%93%D7%99%D7%94%3A%D7%A2%D7%A8%D7%9B%D7%99%D7%9D_%D7%9C%D7%90_%D7%A7%D7%99%D7%99%D7%9E%D7%99%D7%9D_%D7%95%D7%9E%D7%95%D7%92%D7%A0%D7%99%D7%9D
            Disallow: /wiki/ויקיפדיה:דפים_לא_קיימים_ומוגנים
            Disallow: /wiki/ויקיפדיה%3Aדפים_לא_קיימים_ומוגנים
            Disallow: /wiki/%D7%95%D7%99%D7%A7%D7%99%D7%A4%D7%93%D7%99%D7%94:%D7%93%D7%A4%D7%99%D7%9D_%D7%9C%D7%90_%D7%A7%D7%99%D7%99%D7%9E%D7%99%D7%9D_%D7%95%D7%9E%D7%95%D7%92%D7%A0%D7%99%D7%9D
            Disallow: /wiki/%D7%95%D7%99%D7%A7%D7%99%D7%A4%D7%93%D7%99%D7%94%3A%D7%93%D7%A4%D7%99%D7%9D_%D7%9C%D7%90_%D7%A7%D7%99%D7%99%D7%9E%D7%99%D7%9D_%D7%95%D7%9E%D7%95%D7%92%D7%A0%D7%99%D7%9D
            "#
            .as_bytes();

            let parser = Compiler::new("test");
            let machine = parser.compile(example_robots).await.unwrap();

            assert_eq!(true, machine.allow("/index.html"));
            assert_eq!(
                false,
                machine.allow("/wiki/ויקיפדיה:ערכים_לא_קיימים_ומוגנים")
            );
        });
    }
}
