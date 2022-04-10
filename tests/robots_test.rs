use cylon::{self, Compiler};

struct TestCases {
    name: &'static str,
    robots: &'static str,
    ua_token: &'static str,
    input_path: &'static str,
    expected_result: bool,
}

static TEST_CASES: &[TestCases] = &[
    TestCases {
        name: "SimpleRobot",
        robots: r#"
        User-agent: *
        Disallow: /
        "#,
        ua_token: "fakeBot",
        input_path: "/foo",
        expected_result: false,
    },
    TestCases {
        name: "EmptyRobot",
        robots: r#""#,
        ua_token: "fakeBot",
        input_path: "/",
        expected_result: true,
    },
    TestCases {
        name: "DirectiveError",
        robots: r#"
        foo: *
        Bar: /
        "#,
        ua_token: "fakeBot",
        input_path: "/foo",
        expected_result: true,
    },
    TestCases {
        name: "DirectiveCase",
        robots: r#"
        User-AgENt: *
        DisalLOW: /
        alLOW: /foo
        "#,
        ua_token: "fakeBot",
        input_path: "/foo",
        expected_result: true,
    },
    TestCases {
        name: "DirectiveCase2",
        robots: r#"
        User-AgENt: *
        DisalLOW: /
        alLOW: /foo
        "#,
        ua_token: "fakeBot",
        input_path: "/test",
        expected_result: false,
    },
    TestCases {
        name: "DirectiveSpellingMistake",
        robots: r#"
        useragent: *
        disallow: /
        "#,
        ua_token: "fakeBot",
        input_path: "/test",
        expected_result: true,
    },
    TestCases {
        name: "DirectiveSpellingMistake2",
        robots: r#"
        user-agent: *
        disalow: /
        "#,
        ua_token: "fakeBot",
        input_path: "/test",
        expected_result: true,
    },
    TestCases {
        name: "MultiGroup",
        robots: r#"
        user-agent: fooBot

        user-agent: fakeBot
        allow: /z/
        disallow: /
        "#,
        ua_token: "fakeBot",
        input_path: "/z/test",
        expected_result: true,
    },
    TestCases {
        name: "MultiGroup2",
        robots: r#"
        user-agent: fooBot

        user-agent: fakeBot
        allow: /z/
        disallow: /
        "#,
        ua_token: "fakeBot",
        input_path: "/test",
        expected_result: false,
    },
    TestCases {
        name: "MultiGroup3",
        robots: r#"
        user-agent: fakeBot

        user-agent: fooBot
        allow: /z/
        disallow: /
        "#,
        ua_token: "fakeBot",
        input_path: "/test",
        expected_result: false,
    },
    TestCases {
        name: "MultiGroup4",
        robots: r#"
        user-agent: fakeBot

        user-agent: fooBot
        allow: /z/
        disallow: /
        "#,
        ua_token: "fakeBot",
        input_path: "/z/test",
        expected_result: true,
    },
    TestCases {
        name: "NoUserAgent",
        robots: r#"
        Disallow: /
        "#,
        ua_token: "fakeBot",
        input_path: "/foo",
        expected_result: true,
    },
    TestCases {
        name: "UADirectiveNotCaseSensitive",
        robots: r#"
        User-agent: fAKeBOt
        Disallow: /
        "#,
        ua_token: "fakeBot",
        input_path: "/foo",
        expected_result: false,
    },
    TestCases {
        name: "UADirectiveNotCaseSensitive2",
        robots: r#"
        User-agent: fakebot
        Disallow: /
        "#,
        ua_token: "fakeBot",
        input_path: "/foo",
        expected_result: false,
    },
    TestCases {
        name: "SpaceInUA",
        robots: r#"
        User-agent: fake bot
        Disallow: /
        "#,
        ua_token: "fake",
        input_path: "/foo",
        expected_result: true,
    },
    TestCases {
        name: "SpaceInUA2",
        robots: r#"
        User-agent: fake bot
        Disallow: /
        "#,
        ua_token: "bot",
        input_path: "/foo",
        expected_result: true,
    },
    TestCases {
        name: "SpaceInUA3",
        robots: r#"
        User-agent: fake bot
        Disallow: /
        "#,
        ua_token: "fake bot",
        input_path: "/foo",
        expected_result: false,
    },
    TestCases {
        name: "UACaseInsensitive",
        robots: r#"
        User-agent: FAKEbOt
        Disallow: /
        "#,
        ua_token: "fakeBot",
        input_path: "/foo",
        expected_result: false,
    },
    TestCases {
        name: "DefaultGroup",
        robots: r#"
        User-agent: *
        Disallow: /test
        User-agent: noBot
        Disallow: /no
        "#,
        ua_token: "fakeBot",
        input_path: "/test",
        expected_result: false,
    },
    TestCases {
        name: "DefaultGroup",
        robots: r#"
        User-agent: *
        Disallow: /test
        User-agent: noBot
        Disallow: /no
        "#,
        ua_token: "fakeBot",
        input_path: "/no",
        expected_result: true,
    },
    TestCases {
        name: "NoGroup",
        robots: r#"
        User-agent: noBot
        Disallow: /no
        "#,
        ua_token: "fakeBot",
        input_path: "/no",
        expected_result: true,
    },
    TestCases {
        name: "PathCaseSensitive",
        robots: r#"
        User-agent: fakebot
        Disallow: /FOO
        "#,
        ua_token: "fakeBot",
        input_path: "/foo",
        expected_result: true,
    },
    TestCases {
        name: "PathCaseSensitive2",
        robots: r#"
        User-agent: fakebot
        Disallow: /FOO
        "#,
        ua_token: "fakeBot",
        input_path: "/FOO",
        expected_result: false,
    },
    TestCases {
        name: "MostSpecificPathMatch",
        robots: r#"
        User-agent: fakebot
        Allow: /test/page.html
        Disallow: /test
        "#,
        ua_token: "fakeBot",
        input_path: "/test/test",
        expected_result: false,
    },
    TestCases {
        name: "MostSpecificPathMatch2",
        robots: r#"
        User-agent: fakebot
        Allow: /test/page.html
        Disallow: /test
        "#,
        ua_token: "fakeBot",
        input_path: "/test/page.html",
        expected_result: true,
    },
    TestCases {
        name: "MostSpecificPathMatch3",
        robots: r#"
        User-agent: fakebot
        Disallow: /test
        Allow: /test/page.html
        "#,
        ua_token: "fakeBot",
        input_path: "/test/page.html",
        expected_result: true,
    },
    TestCases {
        name: "MostSpecificPathMatch4",
        robots: r#"
        User-agent: fakebot
        Disallow: /test/page.html
        Allow: /test
        "#,
        ua_token: "fakeBot",
        input_path: "/test/test",
        expected_result: true,
    },
    TestCases {
        name: "MostSpecificPathMatch5",
        robots: r#"
        User-agent: fakebot
        Disallow: /test/page.html
        Allow: /test
        "#,
        ua_token: "fakeBot",
        input_path: "/test/page.html",
        expected_result: false,
    },
    TestCases {
        name: "MostSpecificPathMatch6",
        robots: r#"
        User-agent: fakebot
        Allow: /test
        Disallow: /test/page.html
        "#,
        ua_token: "fakeBot",
        input_path: "/test/page.html",
        expected_result: false,
    },
    // https://datatracker.ietf.org/doc/html/draft-koster-rep#section-2.2.2
    TestCases {
        name: "MostSpecificPathMatch7",
        robots: r#"
        User-agent: fakebot
        Allow: /test
        Disallow: /*.html
        "#,
        ua_token: "fakeBot",
        input_path: "/test.html",
        expected_result: false,
    },
    TestCases {
        name: "SamePathAllowWin",
        robots: r#"
        User-agent: fakebot
        Allow: /test
        Disallow: /test
        "#,
        ua_token: "fakeBot",
        input_path: "/test/",
        expected_result: true,
    },
    // https://datatracker.ietf.org/doc/html/draft-koster-rep#section-2.2.2
    TestCases {
        name: "SamePathAllowWin2",
        robots: r#"
        User-agent: fakebot
        Disallow: /test
        Allow: /test
        "#,
        ua_token: "fakeBot",
        input_path: "/test/",
        expected_result: true,
    },
    TestCases {
        name: "SamePathAllowWin2",
        robots: r#"
        User-agent: fakebot
        Allow: /
        Disallow: /
        "#,
        ua_token: "fakeBot",
        input_path: "/test/",
        expected_result: true,
    },
    TestCases {
        name: "SamePathAllowWin3",
        robots: r#"
        User-agent: fakebot
        Disallow: 
        Allow: 
        "#,
        ua_token: "fakeBot",
        input_path: "/test/",
        expected_result: true,
    },
    TestCases {
        name: "EmptyPath",
        robots: r#"
        User-agent: *
        Disallow: /
        "#,
        ua_token: "fakeBot",
        input_path: "",
        expected_result: false,
    },
    TestCases {
        name: "PercentEncode",
        robots: r#"
        User-agent: fakebot
        Disallow: /foo/bar/ツ
        "#,
        ua_token: "fakeBot",
        input_path: "/foo/bar/%E3%83%84",
        expected_result: true,
    },
    // https://datatracker.ietf.org/doc/html/draft-koster-rep#section-2.2.2
    // TestCases {
    //     name: "PersentEncode2",
    //     robots: r#"
    //     User-agent: fakebot
    //     Disallow: /foo/bar/ツ
    //     "#,
    //     ua_token: "fakeBot",
    //     input_path: "/foo/bar/ツ",
    //     expected_result: true,
    // },
    TestCases {
        name: "PercentEncode3",
        robots: r#"
        User-agent: fakebot
        Disallow: /foo/bar/%E3%83%84
        "#,
        ua_token: "fakeBot",
        input_path: "/foo/bar/%E3%83%84",
        expected_result: false,
    },
    TestCases {
        name: "PercentEncode4",
        robots: r#"
        User-agent: fakebot
        Disallow: /foo/bar/%E3%83%84
        "#,
        ua_token: "fakeBot",
        input_path: "/foo/bar/ツ",
        expected_result: true,
    },
    TestCases {
        name: "SpecialCharacters",
        robots: r#"
        User-agent: fakebot
        Disallow: /foo/bar/no
        Allow: /foo/*/no
        "#,
        ua_token: "fakeBot",
        input_path: "/foo/bar/no/page",
        expected_result: false,
    },
    TestCases {
        name: "SpecialCharacters2",
        robots: r#"
        User-agent: fakebot
        Disallow: /foo/bar/no
        Allow: /foo/*/no
        "#,
        ua_token: "fakeBot",
        input_path: "/foo/test/no/page",
        expected_result: true,
    },
    TestCases {
        name: "SpecialCharacters3",
        robots: r#"
        User-agent: fakebot
        Disallow: /foo/test$
        "#,
        ua_token: "fakeBot",
        input_path: "/foo/test",
        expected_result: false,
    },
    TestCases {
        name: "SpecialCharacters4",
        robots: r#"
        User-agent: fakebot
        Disallow: /foo/test$
        "#,
        ua_token: "fakeBot",
        input_path: "/foo/test/",
        expected_result: true,
    },
    TestCases {
        name: "SpecialCharacters5",
        robots: r#"
        User-agent: fakebot
        Disallow: /foo/test#comment
        "#,
        ua_token: "fakeBot",
        input_path: "/foo/test",
        expected_result: false,
    },
    TestCases {
        name: "SpecialCharacters6",
        robots: r#"
        User-agent: fakebot
        #   Disallow: /foo/test
        "#,
        ua_token: "fakeBot",
        input_path: "/foo/test",
        expected_result: true,
    },
];

/// Test result from allow function.
/// Many cases are tested using differnet robots.txt (see TEST_CASES variable)
#[test]
fn test_robots() {
    let mut nb_tests_failed: u32 = 0;

    //Loop on tests cases
    for test in TEST_CASES.iter() {
        println!("Running test : {}", test.name);

        // Compile and test path
        let compiler = Compiler::new(test.ua_token);
        if let Ok(cylon) = tokio_test::block_on(compiler.compile(test.robots.as_bytes())) {
            let output = cylon.allow(test.input_path);
            if output != test.expected_result {
                println!("{} test : failed allow comparaison", test.name);
                nb_tests_failed += 1;
            }
        } else {
            println!("{} test : failed compile robot", test.name);
            nb_tests_failed += 1;
        }
    }

    println!("{} tests runned", TEST_CASES.len());
    assert!(
        nb_tests_failed == 0,
        "{} tests failed, please check log",
        nb_tests_failed
    );
}
