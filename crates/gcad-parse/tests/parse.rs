use gcad_parse::{Scope, eval, eval_point, parse_line, parse_program, split_top_level};

fn scope() -> Scope {
    Scope::from([
        ("w".to_string(), 40.0),
        ("p.x".to_string(), 3.0),
        ("p.y".to_string(), -2.0),
    ])
}

#[test]
fn program_errors_name_the_source_line() {
    let error = parse_program("rect 10 10\n\n# note\nbase: \n").unwrap_err();
    assert_eq!(error.to_string(), "line 4: label without an operation");
    let error = parse_program("rect 10 10\ntext \"open\n").unwrap_err();
    assert_eq!(error.to_string(), "line 2: a quote is not closed");
}

#[test]
fn program_lines_keep_their_numbers_past_blanks_and_comments() {
    let lines = parse_program("\n# a\nrect 1 2\n\n  circle 3\n").unwrap();
    let numbers: Vec<usize> = lines.iter().map(|l| l.number).collect();
    assert_eq!(numbers, vec![3, 5]);
    assert_eq!(lines[1].text, "circle 3");
}

#[test]
fn labels_and_operations_must_be_identifiers() {
    let error = parse_line(7, "9x: rect 1 1").unwrap_err();
    assert_eq!(error.to_string(), "line 7: `9x` is not a valid label");
    let error = parse_line(2, "re-ct 1 1").unwrap_err();
    assert_eq!(
        error.to_string(),
        "line 2: `re-ct` is not an operation name"
    );
}

#[test]
fn quotes_join_words_anywhere_in_a_word() {
    let line = parse_line(1, "text font=\"DejaVu Sans\" \"a  b\"").unwrap();
    assert_eq!(
        line.named,
        vec![("font".to_string(), "DejaVu Sans".to_string())]
    );
    assert_eq!(line.positional, vec!["a  b"]);
}

#[test]
fn precedence_and_unary_signs() {
    let s = scope();
    assert_eq!(eval("1+2*3", &s).unwrap(), 7.0);
    assert_eq!(eval("(1+2)*3", &s).unwrap(), 9.0);
    assert_eq!(eval("8/4/2", &s).unwrap(), 1.0);
    assert_eq!(eval("--w", &s).unwrap(), 40.0);
    assert_eq!(eval("-+-2", &s).unwrap(), 2.0);
    assert_eq!(eval("1+1>1", &s).unwrap(), 1.0);
}

#[test]
fn functions_work_in_degrees() {
    let s = scope();
    assert!((eval("sin(30)", &s).unwrap() - 0.5).abs() < 1e-12);
    assert!((eval("atan2(1,1)", &s).unwrap() - 45.0).abs() < 1e-12);
    assert_eq!(eval("max(1,w,3)", &s).unwrap(), 40.0);
    assert_eq!(eval("min(4)", &s).unwrap(), 4.0);
    assert_eq!(eval("hypot(3,4)", &s).unwrap(), 5.0);
    assert_eq!(eval("round(2.5)+floor(-0.5)+ceil(0.2)", &s).unwrap(), 3.0);
    assert!((eval("pi", &s).unwrap() - std::f64::consts::PI).abs() < 1e-15);
}

#[test]
fn bad_expressions_say_what_is_wrong() {
    let s = scope();
    let message = |e: &str| eval(e, &s).unwrap_err().to_string();
    assert!(message("sqrt(1,2)").contains("takes one number"));
    assert!(message("sqrt(-1)").contains("is not a number"));
    assert!(message("cube(2)").contains("no function `cube`"));
    assert!(message("max()").contains("at least one number"));
    assert!(message("(1+2").contains("missing `)`"));
    assert!(message("1+").contains("ends early"));
    assert!(message("1(2)").contains("unexpected `(` in `1(2)`"));
    assert!(message("1 2").contains("unexpected ` `"));
    assert!(message("2$").contains("unexpected `$`"));
    assert!(message("1!2").contains("not an operator"));
    assert!(message("1..2").contains("is not a number"));
}

#[test]
fn dotted_names_read_from_the_scope() {
    let s = scope();
    assert_eq!(eval("p.x*p.y", &s).unwrap(), -6.0);
    assert_eq!(eval_point("p", &s).unwrap(), (3.0, -2.0));
    assert!(eval_point("1,2,3", &s).is_err());
}

#[test]
fn splits_only_outside_brackets() {
    assert_eq!(
        split_top_level("max(1,2),3,(4,5)", ','),
        vec!["max(1,2)", "3", "(4,5)"]
    );
    assert_eq!(split_top_level("", ','), vec![""]);
    assert_eq!(split_top_level("a;b", ';'), vec!["a", "b"]);
}
