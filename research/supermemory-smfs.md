# Research: Supermemory and smfs, fit for bawn / Brisk

Date 2026-10-25. Research only: nothing was installed, run or cloned, and none of Toby's data was sent anywhere. Sources are primary only: `github.com/supermemoryai/{smfs,supermemory,claude-supermemory}` (README, API metadata, file tree, a few raw source files), `supermemory.ai/docs/*.md` and `supermemory.ai/pricing`. Pages were read through WebFetch, which summarises with a small model, so exact wording and numbers could be off. Re-check anything load-bearing. [U] = not verified from a primary source.

## TL;DR

- **(a) Personal Claude Code memory:** not worth it as a replacement. The hosted plugin sends conversations and tool use to Supermemory's cloud. Your file-based memory is local, readable and free.
- **(b) Brisk agent-orchestrator memory:** no for the hosted product, since it breaks "secrets never leave bawn". Maybe later with `supermemory local` on localhost, but its server binary is closed source. Revisit when the orchestrator exists.
- **(c) Searchable layer over notes/research:** the only interesting fit, and only as a throwaway experiment. smfs is young (v0.0.5, 5 human authors, an open data-loss bug) and uploads every file to the cloud by default.
- **(d) Not worth it:** the correct default today. Write a local grep/ripgrep or SQLite-FTS index into Brisk if you want search.
- **Biggest facts:** smfs is MIT Rust but is only a client. It needs a Supermemory API key and server. The local server is a proprietary binary under a "lite license", and it still depends on an external LLM for memory extraction, which can be Ollama.

## 1. What Supermemory is

- **Product:** "context infrastructure for AI agents": a memory and retrieval API plus connectors (Drive, Gmail, Notion, GitHub), user profiles and a hosted MCP server.
- **What "memory" means technically (docs, how-it-works):**
  - Memories are extracted facts stored in a temporal graph, with vector, full-text and graph indexes built in.
  - A second phase ("dreaming") derives new facts and updates old ones.
  - A "profile" is a static + dynamic summary built from memories.
  - Document chunks are stored alongside, and namespaces (container tags) isolate users or projects.
  - The docs name no LLM or embedding model for the hosted service and give no forgetting or contradiction policy ("learning model decides").
- **API/SDKs:** REST `/v3/documents`, `/v4/search`, `/v4/profile`. SDKs on npm and PyPI. No Rust SDK found [U]. smfs talks to the REST API directly with `reqwest`, so a Brisk Rust client would be a thin wrapper.
- **Open source vs hosted:**
  - `supermemoryai/supermemory` is MIT, TypeScript, about 31k stars, pushed today. It holds the SDKs, apps and skills, **not** the server.
  - Docs say the self-hosted server binary is "built from a separate, non-public codebase". [Self-hosting overview]
- **Self-hosting:**
  - "Supermemory local" is a single binary on `localhost:6767`, data in `~/.supermemory`, install via `curl | bash` or `npx supermemory local`.
  - It is free "within its lite license limit". The limit is not stated in the pages read [U].
  - Embeddings run locally by default (bge-base-en-v1.5, 768d).
  - Memory extraction needs an LLM you supply: OpenAI, Anthropic, Gemini, Groq, or any OpenAI-compatible endpoint such as Ollama or llama.cpp.
  - PDFs need a Gemini key, images need Gemini or Vertex, and web pages go to Firecrawl.
  - Telemetry is **on by default** (`SUPERMEMORY_DISABLE_TELEMETRY=1` turns it off). Update checks are off with `SUPERMEMORY_NO_UPDATE_CHECK=1`.
  - Missing from local: connectors and the Supermemory MCP. Extraction quality depends on your model, and data does not migrate to the cloud.
  - The pricing page says self-hosting is a Scale/Enterprise feature, which sits oddly beside the "lite license" free local binary. The relationship is unclear [U].
- **Pricing (pricing page):** Free $0 with $5 credits; Pro $19 with $20; Max $100 with $130; Scale $399 with $600 (SOC 2, HIPAA BAA); Enterprise custom, including air-gapped. Usage is metered per token and per query, and no hard caps are listed. Free-tier credit burn rate for a heavy subagent user is unknown [U].

## 2. What smfs is

- **Name:** not stated in the README. The code's config dir is `ai.supermemory.supermemoryfs`, so "Supermemory FS" is a safe reading, but that is an inference.
- **What it does:** exposes a Supermemory container as a directory. Files read from a local SQLite cache. Writes are pushed to the cloud in the background. A pull runs every 30 s by default.
  - Only files under configured "memory paths" go through fact extraction. Other files are stored but not processed.
  - After `smfs init` installs a shell wrapper, a flagless `grep` inside a mount becomes semantic search on the server. `grep` with any flag stays literal.
  - A virtual `profile.md` at the mount root holds the summary.
- **Backends:** FUSE on Linux (Docker needs `/dev/fuse` and `SYS_ADMIN`), NFSv3 on macOS. Also a TypeScript and Python "bash" package that gives agents a virtual shell in serverless runtimes. Not relevant to bawn.
- **Language and layout:** Rust workspace (`smfs`, `smfs-core`), Rust 1.80+, MIT. Modules: `api/`, `cache/` (SQLite), `sync/` (push/pull), `mount/` (fuse, nfs), `daemon/`.
- **Install:** `curl -fsSL https://smfs.ai/install | bash`.
  - It fetches the binary and `manifest.json` from GitHub Releases.
  - It checks a SHA-256 from the same release, with no signature.
  - It never calls sudo (as read in `install.sh`), and unpacks via `smfs install` into `~/.local/bin` (per README).
- **Maturity:**

| Item | Value |
|---|---|
| Created | 2026-04-14 |
| Stars / forks | 490 / 36 |
| Latest release | v0.0.5 (2026-05-07); the `bash-v0.0.32` npm-side tag bumped 2026-06-12 |
| Commits on `main` | 133; the last ones I saw were 2026-06-12 |
| `pushed_at` | 2026-10-02 (likely a PR branch) |
| Open | 7 (1 issue, 6 PRs) |
| Distinct authors in last 15 commits | 7, of which 5 human and 2 bots (one is a Claude bot) |
| Release cadence | 8 releases in the first 2 weeks, then nothing since May |

  - Bus factor: 2 or 3 people at one company, with commits mainly from Dhravya, Prasanna721 and MaheshtheDev.
  - Stalled: PRs #23-#25 from 2026-08-18 are unmerged, and there has been no CLI release in about 5 months.
- **Known bug:** issue #18 (open since 2026-05-15) says the push worker clears `dirty_since` unconditionally, so a pull can overwrite local edits with stale server content. The fix PR #19 is open and unmerged. For a notes folder this is a data-loss risk.

## 3. Where data lives and what leaves the machine

- **Hosted (default):**
  - Everything written into the mount is uploaded: the full file content via `POST /v3/documents` or `/v3/documents/file`. Memory extraction and embeddings then run on Supermemory's servers.
  - Auth is a bearer API key, stored in `credentials.json` at mode 0600. The temp file briefly exists with default mode (source read), and `/tmp/supermemoryfs-config` is a fallback location if the config dir can't be found.
  - No retention, encryption-at-rest or deletion-guarantee statements in the pages read [U]. SOC 2 and HIPAA are advertised only on the Scale tier.
- **smfs client itself:** no telemetry found in `credentials.rs` and `api/mod.rs`, and no telemetry file in the tree. Write requests tag `metadata.source` and `lastEditedBy`. I did not read the whole codebase, so "no telemetry" holds only for those files [U].
- **Fully local/offline?**
  - smfs has `--api-url` / `SUPERMEMORY_API_URL`, so it can point at `supermemory local`. The README gives no self-host steps for smfs [U], and I found no doc that confirms smfs works against the local server [U].
  - `--ephemeral` (in-memory cache) and `--no-sync` (no pulls) exist, but local writes still push.
  - The local server can run offline with a local LLM, but extraction quality is then your model's, not theirs.
- **Claude Code plugin** (`claude-supermemory`, now `supermemory@supermemory-plugins`):
  - Node 18+ hooks. Auto-capture saves conversations and "important tool usage" in the background to personal and repo-scoped (team) containers.
  - At most 5 memories (default) are injected before turns. `signalExtraction` can limit capture to turns containing keywords.
  - The docs do not say what exact payload is sent or what is redacted.
  - The repo has **no license** (`license: null`), 2.8k stars, active.
  - The local-server route is `SUPERMEMORY_API_URL=http://localhost:6767` or `baseUrl` in the project config.
- **Hosted MCP** (`mcp.supermemory.ai/mcp`): OAuth, tools `search_memory`, `get_profile`, `add_memory`, `forget_memory`, `list_documents` and others. It has no self-host option.

## 4. Integration surfaces for Toby

- **Claude Code:**
  - Plugin (hooks plus `/supermemory:*` commands) or the hosted MCP. Both are cloud unless you run the local server, and the local server has no MCP.
  - smfs also writes a managed block into `~/.claude/CLAUDE.md` (and Codex/Gemini equivalents) when you mount. It tells the agent to use `smfs grep` instead of grep/rg/find under the mount path, and `smfs unmount` removes it. **This is your private global file with the sudoq rules**, so any tool that edits it needs a backup first.
- **Rust / HTTP:** the REST API is simple (documents, search, profile), so Brisk could call it with `reqwest` in about 100 lines. There is no official Rust SDK [U]. smfs's `smfs-core` crate is not published to crates.io as far as I saw [U].
- **Svelte/TS:** the npm SDK exists. In Brisk it would belong server-side, not in the browser, because it needs the API key.
- **Fit with Brisk's rules:** "tailnet-only, no accounts, secrets never leave bawn" conflicts with a hosted account, API key and uploaded content. The local server would fit the rule, but it is closed source with a license cap, so it fails "pure Rust where practical".

## 5. Security and privacy red flags

1. **Secrets in memories.** Auto-capture of conversations and tool use will include API keys, `.env` contents, tailnet names and sudoq output. The docs describe no redaction [U]. This is the main hazard for the hosted plugin.
2. **Prompt injection through retrieved memory.** Anything saved (web pages Claude read, a malicious README) is later re-injected into context as "memory". Memory is a persistent injection channel, and the injected profile format is not documented. Treat recalled text as untrusted. The smfs `CLAUDE.md` block is also a steer to prefer `smfs grep` output over your own tools.
3. **Supply chain.** `curl | bash` installs; the checksum lives in the same release as the binary, with no signature. The local server install is also `curl | bash`, and its binary is closed. Issue #26 moves CI to a third-party runner.
4. **Privilege.** FUSE on bawn would need `fusermount`/`user_allow_other` config (root, via sudoq) [U exact steps]. Docker needs `SYS_ADMIN`.
5. **Data integrity.** Bug #18 above. Also pulls every 30 s from the server mean the "source of truth" is the cloud.
6. **Telemetry.** The local server has it on by default. The hosted service's own telemetry is not documented [U].
7. **Shell wrapper.** `smfs init` replaces `grep` behaviour in the shell inside mounts. It is confusing for scripts and agents, though flags fall through.

## 6. Compared with Claude Code's built-in memory

| | Claude Code file memory (today) | Supermemory plugin / smfs |
|---|---|---|
| Storage | Markdown plus an index, per project, `~/.claude/projects/*/memory/` | Cloud graph and vectors (or a local closed binary) |
| Network | None | Every capture and recall |
| Readable and editable | Yes, plain text, can be `git`-tracked | Mostly through API, `profile.md` view; facts are derived by an LLM |
| Retrieval | Index loaded each session; grep | Semantic search, temporal facts, profile |
| Capture | Deliberate (the agent or you writes it) | Automatic (signal-based or all turns) |
| Scale | Hundreds of short notes | Thousands of documents, PDFs, images |
| Cost | Free | Credits, or your own LLM for local |
| Undo | Delete a file | `smfs unmount`, delete documents via API, uninstall the plugin; deletion guarantees unknown [U] |
| Failure mode | Stale notes | Wrong or hallucinated "facts", cloud leakage, injected recall |

What Supermemory adds that file memory lacks: semantic recall across a large corpus, automatic fact updates over time, and multimodal search. For a hobbyist with a few dozen memory files, a grep over markdown covers most of it.

## 7. Verdict per use case

- **(a) Personal Claude Code memory: no.** Auto-capture plus the cloud conflicts with local-first, and it adds an injection channel. If curious, use the local server and `signalExtraction`, not the hosted default. Keep the file memory as the source of truth.
- **(b) Brisk orchestrator memory: not now.**
  - The orchestrator's needs (per-agent scratch, run logs, "what did agent X decide") are structured and small. SQLite plus FTS5 in Rust gives that with no daemon.
  - If semantic recall is wanted later, embed locally (for example a small ONNX model) behind a trait. A swappable Supermemory-local backend is an option, but only if the `lite license` and closed binary are acceptable [U terms].
- **(c) Searchable notes/research layer: the only plausible trial, low confidence.**
  - Good fit: read-mostly research folders with PDFs, where semantic grep helps.
  - Bad fit: anything you edit, because of bug #18, and anything sensitive, because of the upload.
  - Prefer `ripgrep` plus, if needed, a local embedding index. Try smfs only on a copy of public material.
- **(d) Not worth it: yes, this is the default answer** until (i) smfs ships a release after v0.0.5 with #18 fixed, and (ii) there is a documented local-server path for smfs. Both checks are cheap to repeat later.

## 8. Try it safely (only if you want to)

Goal: see semantic grep working on junk data, with zero exposure.

1. **Use the free hosted tier with a throwaway account, or the local server, never your real work.** Local server: run it as a user systemd unit bound to 127.0.0.1 with `SUPERMEMORY_DISABLE_TELEMETRY=1` and `SUPERMEMORY_NO_UPDATE_CHECK=1`. Point extraction at Ollama only if it is already on bawn [U]. Read the install script first (`curl -o`, then `less`) instead of piping it.
2. **Install smfs** to `~/.local/bin` by downloading the release binary by hand and verifying the SHA-256 in `manifest.json`. Better, build from a tagged clone with `cargo build --release` (Rust 1.80+, in its own directory). No root for the build.
3. **Before mounting,** back up `~/.claude/CLAUDE.md` (smfs edits it) and check that FUSE is usable by your user. Anything needing root goes through `sudoq`, one batched request. Do not touch bond0 or NetworkManager.
4. **First data:** 5-10 public files you do not mind losing, such as copies of upstream docs. No notes, no tokens, no `.env`. Mount the copy with `--no-sync` first, then `--memory-paths` limited to one folder.
5. **Test:** semantic grep versus `rg`, how long extraction takes, whether `profile.md` is sane. Watch outbound connections with `ss -tp` (or `strace -f -e connect` on the daemon) while it runs.
6. **Undo:** `smfs unmount`, `smfs logout`, delete `~/.local/bin/smfs` and the config dir (`~/.config/supermemoryfs*`) [U exact path], restore the saved `~/.claude/CLAUDE.md`, remove the `smfs init` shell wrapper lines from your rc file, delete the container's documents through the API (`DELETE /v3/documents/bulk`) or delete the throwaway account, then stop and remove the local server and `~/.supermemory`.

Do not: install the Claude Code plugin with auto-capture on your real projects during the trial.

## 9. Questions I could not answer from primary sources

1. What is the "lite license" limit for `supermemory local`, and what do its terms allow? The pricing page says self-hosting is for Scale/Enterprise.
2. Does smfs work end-to-end against `supermemory local`? There is no documented smfs self-host procedure.
3. What does smfs stand for? The README does not say.
4. What exact payload does the Claude Code plugin send (full turns, tool outputs, file contents), and is anything redacted?
5. Hosted retention, encryption at rest, deletion guarantees and training use of data. The pages read say nothing, and I did not find a privacy policy page.
6. Telemetry of the hosted service and of the whole smfs codebase (only two source files were read).
7. Default memory-path scope per container, and file size or count limits.
8. How the hosted service resolves contradictions and forgetting (docs say only that the model decides).
9. The format in which memories and the profile are injected into context, which decides how exposed you are to injection.
10. Whether the last-commit picture is complete: `main` shows June, while `pushed_at` says 2026-10-02. I did not inspect branches.
11. FUSE prerequisites on bawn (`fuse3`, `user_allow_other`) and whether `smfs install` writes anywhere besides `~/.local/bin`.
12. Whether `smfs-core` is usable as a library crate from Brisk (not on crates.io as far as I saw).
13. The license of `claude-supermemory` (the repo reports none, so reuse rights are unclear).
