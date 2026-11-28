//! `rtk git show` of an object that is not a commit, reached by a name the blob pre-filter
//! cannot recognise from its spelling (#5330): a lightweight tag on a blob, an annotated tag
//! on a blob, and a tree. These go down the commit-diff path, which used to decode git's
//! stdout lossily and print it with a trailing newline. They must now be byte-identical to
//! `git show`, while a real commit is still compacted.

use std::path::Path;
use std::process::{Command, Output};

mod common;

fn git(repo: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new("git");
    common::isolate_git(&mut cmd);
    cmd.current_dir(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(["-c", "core.autocrlf=false"])
        .args(args);
    let out = cmd.output().expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn rtk_show(repo: &Path, args: &[&str]) -> Output {
    let mut cmd = common::rtk_command();
    common::isolate_git(&mut cmd);
    cmd.current_dir(repo).arg("git").arg("show").args(args);
    cmd.output().expect("run rtk git show")
}

/// The repository from the issue: a Latin-1 blob ending in a newline, a Latin-1 blob without
/// one, a lightweight tag `lb` on the first and an annotated tag `tb` on the second.
fn setup_repo() -> tempfile::TempDir {
    let dir = common::tempdir();
    let repo = dir.path();
    git(repo, &["init", "-q"]);
    std::fs::write(repo.join("a.txt"), b"caf\xe9 line\nplain\n").expect("write a.txt");
    std::fs::write(repo.join("b.txt"), b"caf\xe9 end").expect("write b.txt");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "c"]);
    let blob_a = rev_parse(repo, "HEAD:a.txt");
    let blob_b = rev_parse(repo, "HEAD:b.txt");
    git(repo, &["tag", "lb", &blob_a]);
    git(repo, &["tag", "-a", "-m", "t", "tb", &blob_b]);
    dir
}

fn rev_parse(repo: &Path, rev: &str) -> String {
    let out = git(repo, &["rev-parse", rev]);
    String::from_utf8(out.stdout)
        .expect("object id is ASCII")
        .trim()
        .to_string()
}

#[test]
fn non_commit_objects_are_byte_exact() {
    let dir = setup_repo();
    let repo = dir.path();
    let tree = rev_parse(repo, "HEAD^{tree}");

    let cases: Vec<Vec<&str>> = vec![
        vec!["lb"],
        vec!["refs/tags/lb"],
        vec!["tb"],
        vec!["HEAD^{tree}"],
        vec![&tree],
        // `-s` returns before the diff step: a blob or a tree is still caught by the
        // summary step's comparison with plain `git show`.
        vec!["-s", "lb"],
        vec!["-s", "HEAD^{tree}"],
        // `--oneline` outranks RTK's own format in the summary and diff steps alike.
        vec!["--oneline", "tb"],
        vec!["-p", "tb"],
        // Two non-commit objects: git concatenates them, and so must rtk.
        vec!["lb", "HEAD^{tree}"],
        // The summary-only route writes git's bytes too.
        vec!["--stat", "lb"],
        vec!["--stat", "tb"],
        vec!["--format=%s", "tb"],
    ];

    for args in &cases {
        let mut native = Command::new("git");
        common::isolate_git(&mut native);
        let native = native
            .current_dir(repo)
            .arg("show")
            .args(args)
            .output()
            .expect("run git show");
        let rtk = rtk_show(repo, args);
        assert_eq!(
            rtk.status.code(),
            native.status.code(),
            "exit code differs for git show {args:?}"
        );
        assert_eq!(
            rtk.stdout, native.stdout,
            "rtk git show {args:?} is not byte-identical to git show"
        );
    }
}

#[test]
fn a_commit_is_still_compacted() {
    let dir = setup_repo();
    let repo = dir.path();

    let mut native = Command::new("git");
    common::isolate_git(&mut native);
    let native = native
        .current_dir(repo)
        .args(["show", "HEAD"])
        .output()
        .expect("run git show HEAD");
    let rtk = rtk_show(repo, &["HEAD"]);
    assert!(rtk.status.success());
    let rtk_text = String::from_utf8_lossy(&rtk.stdout);
    // The one-line summary RTK prints in place of git's `commit <sha>` header.
    assert!(
        !rtk_text.starts_with("commit "),
        "git show HEAD should be compacted, got:\n{rtk_text}"
    );
    assert!(
        rtk_text.contains(" c ("),
        "summary line missing:\n{rtk_text}"
    );
    assert_ne!(rtk.stdout, native.stdout);
}
