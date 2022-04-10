#[cfg(feature = "crawl-delay")]
use std::cmp::Ordering;

use std::collections::{BTreeSet, VecDeque};

use serde_derive::{Deserialize, Serialize};

const EOW_BYTE: u8 = 36; // '$'
const WILDCARD_BYTE: u8 = 42; // '*'

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule<'a> {
    Allow(&'a [u8]),
    Disallow(&'a [u8]),
    #[cfg(feature = "crawl-delay")]
    Delay(&'a [u8]),
}

impl<'a> Rule<'a> {
    fn inner(&self) -> &[u8] {
        match self {
            Rule::Allow(inner) => inner,
            Rule::Disallow(inner) => inner,
            #[cfg(feature = "crawl-delay")]
            Rule::Delay(inner) => inner,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
enum Accept {
    Allow,
    Disallow,
    #[cfg(feature = "crawl-delay")]
    Delay,
}

/// A Node represents a state in the NFA. All nodes are
/// either an 'allow' or 'disallow' state, meaning if the
/// input matches the state we should either allow that
/// URL to be crawled or forbid it from being crawled.
/// In addition, all states have a wildcard transition to
/// another state (or itself) if none of the provided
/// edges match the input.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
struct Node {
    accept: Accept,
    edges: Vec<(u8, usize)>,
    weight: usize,
    wildcards: Vec<usize>,
}

impl Node {
    fn new(accept: Accept, weight: usize) -> Self {
        Self {
            accept,
            edges: vec![],
            weight,
            wildcards: vec![],
        }
    }

    fn add_edge(&mut self, edge: u8, state: usize) {
        self.edges.push((edge, state));
    }

    fn add_wildcard(&mut self, state: usize) {
        self.wildcards.push(state);
    }

    fn follow_edges(&self, edge: u8, fallback: usize) -> Vec<usize> {
        let mut out = vec![];
        for (char, state) in &self.edges {
            if *char == edge {
                out.push(*state)
            }
        }
        for state in &self.wildcards {
            out.push(*state);
        }
        if self.wildcards.is_empty() {
            out.push(fallback)
        }
        out
    }

    fn allow(&self) -> bool {
        match self.accept {
            Accept::Allow => true,
            Accept::Disallow => false,
            #[cfg(feature = "crawl-delay")]
            Accept::Delay => true,
        }
    }

    /// Re-map the node's weight based on the accept state. This
    /// makes it easier to guarantee Allow states always break
    /// ties against Disallow states.
    fn normalized_weight(&self) -> usize {
        match self.accept {
            Accept::Allow => 1 + 2 * self.weight,
            Accept::Disallow => 2 * self.weight,
            #[cfg(feature = "crawl-delay")]
            Accept::Delay => 1 + 2 * self.weight,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct QueueItem<'a> {
    parent_prefix: &'a [u8],
    parent_state: usize,
    epsilon_state: Option<usize>,
}

impl<'a> QueueItem<'a> {
    fn new(parent_prefix: &'a [u8], parent_state: usize) -> Self {
        Self {
            parent_prefix,
            parent_state,
            epsilon_state: None,
        }
    }
}

impl<'a> Default for QueueItem<'a> {
    fn default() -> Self {
        Self::new(b"", 0)
    }
}

/// A Cylon is an NFA that recognizes rules from a compiled robots.txt
/// file. By providing it a URL path, it can decide whether or not
/// the robots file that compiled it allows or disallows that path.
///
/// The performance is on average O(n ^ k), where n is the length of the path
/// and k is the average number of transitions from one prefix. This
/// exponontial runtime is acceptable in most cases because k tends to be
/// very small.
///
/// Contrast that with the naive approach of matching each rule individually.
/// If you can match a rule in O(n) time and there are p rules of length q,
/// then the performance will be O(n * p * q). However the NFA is likely more
/// efficient, because it can avoid matching the same prefix multiple times.
/// If there are x prefixes and each prefix is used y times, then the naive
/// approach must make O(x * y) comparisons whereas the NFA only makes O(y)
/// comparisons.
///
/// In general robots.txt files have a lot of shared prefixes due to the
/// nature of URLs. That is why the pre-compiled NFA will be faster in
/// most cases. However there is an upfront cost of compiling the NFA which is
/// not present when doing naive matching. That cost can be amortized by
/// caching the compiled Cylon for subsequent uses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cylon {
    states: Vec<Node>,
    #[cfg(feature = "crawl-delay")]
    delay: Option<u64>,
}

impl Cylon {
    #[cfg(feature = "crawl-delay")]
    pub fn delay(&self) -> Option<u64> {
        self.delay
    }

    /// Match whether the rules allow or disallow the target path.
    pub fn allow<T: AsRef<[u8]>>(&self, path: T) -> bool {
        let path = path.as_ref();
        let path = if path.is_empty() { b"/" } else { path };
        let mut current_states = BTreeSet::new();
        current_states.insert(0);

        for edge in path {
            let mut next_states = BTreeSet::new();

            for s in current_states {
                if let Some(state) = self.states.get(s) {
                    next_states.extend(state.follow_edges(*edge, s));
                }
            }

            current_states = next_states;
        }

        let best_match = current_states
            .into_iter()
            .flat_map(|s| self.states.get(s))
            .max_by_key(|n| n.normalized_weight());

        match best_match {
            Some(state) => state.allow(),
            None => true,
        }
    }

    pub fn compile(mut rules: Vec<Rule>) -> Self {
        let mut first = Node::new(Accept::Allow, 0);
        let second = Node::new(Accept::Allow, 0);
        first.add_wildcard(1);

        let mut states: Vec<Node> = vec![first, second];
        let mut queue = VecDeque::new();
        queue.push_back(QueueItem::default());
        rules.sort_by(|a, b| Ord::cmp(a.inner(), b.inner()));

        while let Some(QueueItem {
            parent_prefix,
            parent_state,
            epsilon_state,
        }) = queue.pop_front()
        {
            let mut last_prefix: &[u8] = b"";
            for rule in rules.iter() {
                let prefix = match rule.inner().get(..=parent_prefix.len()) {
                    None => continue,
                    Some(prefix) if !prefix.starts_with(parent_prefix) => continue,
                    Some(prefix) if last_prefix == prefix => continue,
                    Some(prefix) => prefix,
                };

                let is_terminal = prefix == rule.inner();
                let accept_state = match rule {
                    Rule::Allow(..) if is_terminal => Accept::Allow,
                    Rule::Disallow(..) if is_terminal => Accept::Disallow,
                    #[cfg(feature = "crawl-delay")]
                    Rule::Delay(..) if is_terminal => Accept::Allow,
                    _ => states.get(parent_state).unwrap().accept,
                };

                let state = states.len();
                let edge = *prefix.last().unwrap();
                let parent_edge = parent_prefix.last();
                let parent_node = states.get_mut(parent_state).unwrap();
                let mut child_node = Node::new(accept_state, prefix.len());
                let mut wildcard_node = None;
                let mut queue_item = QueueItem::new(prefix, state);

                match edge {
                    WILDCARD_BYTE if parent_edge != Some(&WILDCARD_BYTE) => {
                        child_node.add_wildcard(state);
                        parent_node.add_wildcard(state);

                        if is_terminal {
                            // If this is a terminal node with a different accept
                            // state than its parent, then the parent should
                            // inherit this accept state since the wildcard also
                            // matches its parent.
                            parent_node.accept = accept_state
                        }

                        for (_, e) in parent_node.edges.clone() {
                            // Ensure a wildcard transition from all siblings
                            // to this node, as well as the parent.
                            let sibling_node = states.get_mut(e).unwrap();
                            sibling_node.add_wildcard(state);
                        }
                        // Wildcard matches can match 0 characters, so resolve
                        // an epsilon transition from this nodes parent to this
                        // node's children.
                        queue_item.epsilon_state = Some(parent_state);
                    }
                    WILDCARD_BYTE => {
                        // Avoid the extremely inefficient degenerate case of multiple
                        // repeated wildcard characters by simply ignoring them.
                        last_prefix = prefix;
                        queue_item.parent_state = parent_state;
                        queue.push_back(queue_item);
                        continue;
                    }
                    EOW_BYTE => {
                        parent_node.add_wildcard(state);
                        // Swap the accept states of the parent node, which was
                        // not treated as a terminal state and inherited its parent's.
                        // If we match beyond the EOW we should use the grandparent
                        // accept state instead since that was technically the last match.
                        child_node.accept = parent_node.accept;
                        parent_node.accept = accept_state;
                        parent_node.weight = child_node.weight;
                    }
                    edge => {
                        parent_node.add_edge(edge, state);
                        for e in parent_node.wildcards.iter() {
                            // Inherit the parent's wildcard transitions. If
                            // we cannot match more characters we should jump
                            // back to the parent's wildcard transition.
                            if state != *e && !is_terminal {
                                child_node.add_wildcard(*e);
                            }
                        }

                        if is_terminal {
                            // Any characters after a terminal node should jump to a state
                            // that has no transitions except for a wildcard self-loop.
                            wildcard_node = Some(Node::new(accept_state, prefix.len()));
                            child_node.add_wildcard(state + 1);
                        }
                    }
                }

                if let Some(grandparent_state) = epsilon_state {
                    let grandparent_node = states.get_mut(grandparent_state).unwrap();
                    grandparent_node.add_edge(edge, state);
                }

                last_prefix = prefix;
                states.push(child_node);
                queue.push_back(queue_item);

                if let Some(wildcard_node) = wildcard_node {
                    states.push(wildcard_node);
                }
            }
        }

        #[cfg(feature = "crawl-delay")]
        {
            let mut delays: Vec<Option<u64>> = rules
                .iter()
                .filter(|rule| match rule {
                    Rule::Delay(_) => true,
                    _ => false,
                })
                .map(|r| r.inner())
                .flat_map(|r| std::str::from_utf8(r).ok())
                .map(|r| r.parse::<u64>().ok())
                .collect();
            delays.sort_unstable_by(|a, b| match (a, b) {
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (None, None) => Ordering::Equal,
                (Some(aa), Some(bb)) => aa.cmp(bb),
            });
            Self {
                delay: *delays.get(0).unwrap_or(&None),
                states,
            }
        }

        #[cfg(not(feature = "crawl-delay"))]
        Self { states }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! n {
        ('a' $x:literal $e:expr) => {
            Node {
                accept: Accept::Allow,
                edges: $e,
                weight: $x,
                wildcards: vec![],
            }
        };
        ('a' $x:literal $w:literal, $e:expr) => {
            Node {
                accept: Accept::Allow,
                edges: $e,
                weight: $x,
                wildcards: vec![$w],
            }
        };
        ('a' $x:literal $w:expr, $e:expr) => {
            Node {
                accept: Accept::Allow,
                edges: $e,
                weight: $x,
                wildcards: $w,
            }
        };
        ('d' $x:literal $e:expr) => {
            Node {
                accept: Accept::Disallow,
                edges: $e,
                weight: $x,
                wildcards: vec![],
            }
        };
        ('d' $x:literal $w:literal, $e:expr) => {
            Node {
                accept: Accept::Disallow,
                edges: $e,
                weight: $x,
                wildcards: vec![$w],
            }
        };
        ('d' $x:literal $w:expr, $e:expr) => {
            Node {
                accept: Accept::Disallow,
                edges: $e,
                weight: $x,
                wildcards: $w,
            }
        };
    }

    macro_rules! b {
        ('.') => {
            46
        };
        ('/') => {
            47
        };
        ('a') => {
            97
        };
        ('b') => {
            98
        };
        ('c') => {
            99
        };
        ('d') => {
            100
        };
        ('e') => {
            101
        };
        ('f') => {
            102
        };
        ('s') => {
            115
        };
        ('t') => {
            116
        };
        ('x') => {
            120
        };
        ('y') => {
            121
        };
    }

    #[test]
    fn test_compile_1() {
        // Allow:     /a
        // Disallow:  /abc
        // Allow:     /a*c

        let rules = vec![
            Rule::Allow(b"/a"),
            Rule::Disallow(b"/abc"),
            Rule::Allow(b"/a*c"),
        ];

        let expect_nodes = vec![
            n!('a' 0 1, vec![(b!('/'), 2)]),                        // ''
            n!('a' 0 vec![]),                                       // '' wildcard
            n!('a' 1 1, vec![(b!('a'), 3)]),                        // '/'
            n!('a' 2 vec![4, 5], vec![(b!('b'), 6), (b!('c'), 7)]), // '/a'
            n!('a' 2 vec![]),                                       // '/a' wildcard
            n!('a' 3 5, vec![(b!('c'), 7)]),                        // '/a*'
            n!('a' 3 vec![4, 5], vec![(b!('c'), 9)]),               // '/ab'
            n!('a' 4 8, vec![]),                                    // '/a*c'
            n!('a' 4 vec![]),                                       // '/a*c' wildcard
            n!('d' 4 10, vec![]),                                   // '/abc'
            n!('d' 4 vec![]),                                       // '/abc' wildcard
        ];

        let actual = Cylon::compile(rules);
        assert_eq!(actual.states, expect_nodes);
    }

    #[test]
    fn test_compile_2() {
        // Allow:     /a
        // Disallow:  /a$
        // Disallow:  /ab

        let rules = vec![
            Rule::Allow(b"/a"),
            Rule::Disallow(b"/a$"),
            Rule::Disallow(b"/ab"),
        ];

        let expect_nodes = vec![
            n!('a' 0 1, vec![(b!('/'), 2)]),          // ''
            n!('a' 0 vec![]),                         // '' wildcard
            n!('a' 1 1, vec![(b!('a'), 3)]),          // '/'
            n!('d' 3 vec![4, 5], vec![(b!('b'), 6)]), // '/a$'
            n!('a' 2 vec![]),                         // '/a' wildcard
            n!('a' 3 vec![]),                         // '/a$' wildcard
            n!('d' 3 7, vec![]),                      // '/ab'
            n!('d' 3 vec![]),                         // '/ab' wildcard
        ];

        let actual = Cylon::compile(rules);
        assert_eq!(actual.states, expect_nodes);
    }

    #[test]
    fn test_degenerate_1() {
        let rules = vec![Rule::Allow(b"/****************************")];

        let expect_nodes = vec![
            n!('a' 0 1, vec![(b!('/'), 2)]), // ''
            n!('a' 0 vec![]),                // '' wildcard
            n!('a' 1 vec![1, 3], vec![]),    // '/'
            n!('a' 2 3, vec![]),             // '/*'
        ];

        let actual = Cylon::compile(rules);
        assert_eq!(actual.states, expect_nodes);
    }

    #[test]
    fn test_allow() {
        let rules = vec![
            Rule::Disallow(b"/"),
            Rule::Allow(b"/a"),
            Rule::Allow(b"/abc"),
            Rule::Allow(b"/b"),
        ];

        let machine = Cylon::compile(rules);
        assert_eq!(false, machine.allow("/"));
        assert_eq!(true, machine.allow("/a"));
        assert_eq!(true, machine.allow("/a/b"));
        assert_eq!(true, machine.allow("/a"));
        assert_eq!(true, machine.allow("/abc"));
        assert_eq!(true, machine.allow("/abc/def"));
        assert_eq!(true, machine.allow("/b"));
        assert_eq!(true, machine.allow("/b/c"));
    }

    #[test]
    fn test_priority_1() {
        let rules = vec![Rule::Disallow(b"/a.b"), Rule::Allow(b"/*.b")];

        let machine = Cylon::compile(rules);
        assert_eq!(true, machine.allow("/"));
        assert_eq!(true, machine.allow("/a.b"));
        assert_eq!(true, machine.allow("/b.b"));
    }

    #[test]
    fn test_priority_2() {
        let rules = vec![Rule::Disallow(b"/ab.c"), Rule::Allow(b"/*.c")];

        let machine = Cylon::compile(rules);
        assert_eq!(true, machine.allow("/"));
        assert_eq!(true, machine.allow("/a.c"));
        assert_eq!(true, machine.allow("/b.c"));
        assert_eq!(false, machine.allow("/ab.c"));
    }

    #[test]
    fn test_tricky() {
        let rules = vec![Rule::Disallow(b"/abc"), Rule::Allow(b"/abd")];

        let machine = Cylon::compile(rules);
        assert_eq!(false, machine.allow("/abc"));
        assert_eq!(true, machine.allow("/abd"));
    }

    #[test]
    fn test_allow_match_any() {
        let rules = vec![
            Rule::Allow(b"/"),
            Rule::Disallow(b"/secret/*.txt"),
            Rule::Disallow(b"/private/*"),
        ];

        let machine = Cylon::compile(rules);
        assert_eq!(true, machine.allow("/"));
        assert_eq!(true, machine.allow("/abc"));
        assert_eq!(false, machine.allow("/secret/abc.txt"));
        assert_eq!(false, machine.allow("/secret/123.txt"));
        assert_eq!(true, machine.allow("/secret/abc.csv"));
        assert_eq!(true, machine.allow("/secret/123.csv"));
        assert_eq!(false, machine.allow("/private/abc.txt"));
        assert_eq!(false, machine.allow("/private/123.txt"));
        assert_eq!(false, machine.allow("/private/abc.csv"));
        assert_eq!(false, machine.allow("/private/123.csv"));
    }

    #[test]
    fn test_allow_match_eow() {
        let rules = vec![
            Rule::Allow(b"/"),
            Rule::Disallow(b"/ignore$"),
            Rule::Disallow(b"/foo$bar"),
        ];

        let machine = Cylon::compile(rules);
        assert_eq!(true, machine.allow("/"));
        assert_eq!(true, machine.allow("/abc"));
        assert_eq!(false, machine.allow("/ignore"));
        assert_eq!(true, machine.allow("/ignoreabc"));
        assert_eq!(true, machine.allow("/ignore/abc"));
        // These are technically undefined, and no behavior
        // is guaranteed since the rule is malformed. However
        // it is safer to accept them rather than reject them.
        assert_eq!(true, machine.allow("/foo"));
        assert_eq!(false, machine.allow("/foo$bar"));
    }

    #[test]
    fn test_allow_more_complicated() {
        let rules = vec![
            Rule::Allow(b"/"),
            Rule::Disallow(b"/a$"),
            Rule::Disallow(b"/abc"),
            Rule::Allow(b"/abc/*"),
            Rule::Disallow(b"/foo/bar"),
            Rule::Allow(b"/*/bar"),
            Rule::Disallow(b"/www/*/images"),
            Rule::Allow(b"/www/public/images"),
        ];

        let machine = Cylon::compile(rules);
        assert_eq!(true, machine.allow("/"));
        assert_eq!(true, machine.allow("/directory"));
        assert_eq!(false, machine.allow("/a"));
        assert_eq!(true, machine.allow("/ab"));
        assert_eq!(false, machine.allow("/abc"));
        assert_eq!(true, machine.allow("/abc/123"));
        assert_eq!(true, machine.allow("/foo"));
        assert_eq!(true, machine.allow("/foobar"));
        assert_eq!(false, machine.allow("/foo/bar"));
        assert_eq!(false, machine.allow("/foo/bar/baz"));
        assert_eq!(true, machine.allow("/baz/bar"));
        assert_eq!(false, machine.allow("/www/cat/images"));
        assert_eq!(true, machine.allow("/www/public/images"));
    }

    #[test]
    fn test_matches() {
        // Test cases from:
        // https://developers.google.com/search/reference/robots_txt#group-member-rules

        let machine = Cylon::compile(vec![Rule::Disallow(b"/"), Rule::Allow(b"/fish")]);
        assert_eq!(true, machine.allow("/fish"));
        assert_eq!(true, machine.allow("/fish.html"));
        assert_eq!(true, machine.allow("/fish/salmon.html"));
        assert_eq!(true, machine.allow("/fishheads.html"));
        assert_eq!(true, machine.allow("/fishheads/yummy.html"));
        assert_eq!(true, machine.allow("/fish.php?id=anything"));
        assert_eq!(false, machine.allow("/Fish.asp"));
        assert_eq!(false, machine.allow("/catfish"));
        assert_eq!(false, machine.allow("/?id=fish"));

        let machine = Cylon::compile(vec![Rule::Disallow(b"/"), Rule::Allow(b"/fish*")]);
        assert_eq!(true, machine.allow("/fish"));
        assert_eq!(true, machine.allow("/fish.html"));
        assert_eq!(true, machine.allow("/fish/salmon.html"));
        assert_eq!(true, machine.allow("/fishheads.html"));
        assert_eq!(true, machine.allow("/fishheads/yummy.html"));
        assert_eq!(true, machine.allow("/fish.php?id=anything"));
        assert_eq!(false, machine.allow("/Fish.asp"));
        assert_eq!(false, machine.allow("/catfish"));
        assert_eq!(false, machine.allow("/?id=fish"));

        let machine = Cylon::compile(vec![Rule::Disallow(b"/"), Rule::Allow(b"/*.php")]);
        assert_eq!(true, machine.allow("/filename.php"));
        assert_eq!(true, machine.allow("/folder/filename.php"));
        assert_eq!(true, machine.allow("/folder/filename.php?parameters"));
        assert_eq!(true, machine.allow("/folder/any.php.file.html"));
        assert_eq!(true, machine.allow("/filename.php/"));
        assert_eq!(false, machine.allow("/"));
        assert_eq!(false, machine.allow("/windows.PHP"));

        let machine = Cylon::compile(vec![Rule::Disallow(b"/"), Rule::Allow(b"/*.php$")]);
        assert_eq!(true, machine.allow("/filename.php"));
        assert_eq!(true, machine.allow("/folder/filename.php"));
        assert_eq!(false, machine.allow("/filename.php?parameters"));
        assert_eq!(false, machine.allow("/filename.php/"));
        assert_eq!(false, machine.allow("/filename.php5"));
        assert_eq!(false, machine.allow("/windows.PHP"));

        let machine = Cylon::compile(vec![Rule::Disallow(b"/"), Rule::Allow(b"/fish*.php")]);
        assert_eq!(true, machine.allow("/fish.php"));
        assert_eq!(true, machine.allow("/fishheads/catfish.php?parameters"));
        assert_eq!(false, machine.allow("/Fish.PHP"));
    }
}
