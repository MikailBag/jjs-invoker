workflow "OnPush" {
  on = "push"
  resolves = ["Publish", "Docs", "Test"]
}

action "Check" {
  uses = "docker://mikailbag/jjs-dev:gh-77cbd2700a4e7a04c4205b5425b2e4e7f3625819"
  runs = "cargo build-jjs"
  env = {
    RUST_BACKTRACE = "1"
  }
}

action "Test" {
  uses = "docker://mikailbag/jjs-dev:gh-77cbd2700a4e7a04c4205b5425b2e4e7f3625819"
  runs = "cargo test-jjs"
  needs = ["Check"]
  env = {
      RUST_BACKTRACE = "1"
  }
}

action "Publish" {
  uses = "docker://mikailbag/jjs-dev:gh-77cbd2700a4e7a04c4205b5425b2e4e7f3625819"
  needs = ["Check"]
  runs = "bash ./scripts/publish.sh"
  secrets = ["JJS_DEVTOOL_YANDEXDRIVE_ACCESS_TOKEN"]
  env = {
    RUST_BACKTRACE = "1"
  }
}

action "Docs" {
  uses = "docker://mikailbag/jjs-dev:gh-77cbd2700a4e7a04c4205b5425b2e4e7f3625819"
  needs = ["Check"]
  runs = "cargo run -p devtool -- man"
  secrets = ["GITHUB_TOKEN"]
  env = {
    RUST_BACKTRACE = "1"
  }
}
