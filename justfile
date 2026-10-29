# Thin wrappers over scripts/check.sh steps and the service scripts. `just` lists them.

default:
    @just --list

# Every check, in order (what CI runs)
check:
    scripts/check.sh

# fmt, clippy and nextest only
fast:
    scripts/check.sh fast

# Format Rust and the web app in place
fmt:
    PATH="$HOME/.cargo/bin:$PATH" cargo fmt
    npm --prefix web run fmt

# clippy and the wording check
lint:
    scripts/check.sh clippy wording

# nextest and doc tests
test:
    scripts/check.sh test doc

test-embed:
    scripts/check.sh embed

test-scripts:
    scripts/check.sh scripts

test-web:
    scripts/check.sh web

test-browser:
    scripts/check.sh browser

deny:
    scripts/check.sh deny

pure:
    scripts/check.sh pure

up *args:
    scripts/brisk-up.sh {{args}}

down *args:
    scripts/brisk-down.sh {{args}}

doctor:
    scripts/brisk-service.sh doctor

update:
    scripts/brisk-service.sh update

# Any brisk-service.sh command, e.g. `just svc status`
svc *args:
    scripts/brisk-service.sh {{args}}
