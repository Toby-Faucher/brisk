# Novelty loop (started 2026-10-24, user asleep)

Goal: find an idea for the web Moonlight/game-streaming client that is genuinely novel AND plausibly big.
Stop when: a candidate survives a prior-art search (>=10 queries incl. arXiv/patents/GitHub) AND has a cheap experiment. Then write it up and stop.
Budget: max 4 rounds of agents (6 agents each). Read-only research, no installs, no sudo, nothing built.
Honesty: "not done before" = "I found no prior art in N searches", never "proven novel".

## Round 1 (running): quant/HFT, control+RL, adjacent industries, host pipeline, perception, coding/multipath
Pending reports -> candidates below.

## Candidates (idea | prior art found | experiment | verdict)
Round 1 report 3/6 in (adjacent industries). Needs prior-art pass before any "wow":
- C1 rotation-only timewarp on CLIENT, driven by client's own stick input, host sends rendered camera pose per frame. Agent found no shipped system for flat games. Related: Outatime (needs game changes), Reflex 2 Frame Warp (host-side, mouse), VR timewarp. Experiment: one title, WebGL rotational warp. Risk: needs per-game camera pose; edge smear. Verdict: UNVERIFIED, promising.
- C2 deadline-bounded repair + expiry drop (SRT/RIST/MoQ): standard, not novel.
- C3 OpenIPC-style link-score profile ladder: borrowed, not novel in games.
Report 4/6 in (perception). 
- C4 per-user psychophysics: blind staircase on Toby's own setup fits weights of U = SSIM - dQ - stall - k*lat - nu*sigma_frametime; controller (MPC) then optimizes THAT. Prior art likely: Puffer QoE fit, personalized ABR QoE papers. Need prior-art search; probably "good practice" not "wow".
- C5 client-side instant feedback (rumble/sound/hit flash at input time): cheap, likely exists in some form; verify.
- Perception facts: stick/cursor tracking is strictest (JND ~11-65ms), stall SIZE matters more than frequency, tail frame-time predicts QoE better than mean.
Report 5/6 in (coding/multipath/Tailscale). Facts: Tailscale = one path per peer, DERP is TCP (HOL blocking), peer relays exist (UDP), SO_TXTIME/eBPF useless behind TUN, WebTransport has outgoingMaxAge + sendOrder (spec; Chrome shipping unverified), DCVC-RT not browser-viable.
- C6 parity-over-alternate-path: source via tailnet session, parity via second endpoint (IPv6/port-forward) merged at app level. Browser multipath via two WebTransport sessions: agent found no browser API; prior art for dup/parity on other path exists in MPQUIC drafts (DMTP) but not in browser WebTransport game streaming that I know. Weak: correlated fate on same NIC. Needs search.
- C7 NACK-driven incremental parity (HARQ for video frames) on QUIC datagrams: BC-DIR (V2X sim) is close prior art. Not novel.
Report 6/6 in (control/RL). Lesson: Puffer/Fugu = classical MPC + small in-situ-trained predictor beat end-to-end RL; sim-trained RL (R3Net, Pensieve) fails on real traces.
- C8 (strongest so far) "Fugu transplant": predict per-FRAME delivery-latency PMF (not throughput) from (size, FEC level, OWD, loss, pacing) with ~2x64 MLP trained on Toby's OWN logged path; feed ONE joint MPC over (bitrate, FEC ratio, playout delay) with shared coupling = effective rate; learned part is a bounded RESIDUAL under a classical shadow controller + hard guardrail. Agent found no paper on joint bitrate+FEC+delay MPC/ADMM or MPC/Smith predictor for cloud-gaming. Related: Fugu (VOD chunks), TAROT (FEC grid), Teams DIQL (offline RL, 11% fewer bad calls). Needs prior-art pass: search "per-frame latency distribution prediction cloud gaming", "joint FEC bitrate playout MPC", Parsec/GFN patents.
- C9 Genet-style adversarial trace curriculum vs own baseline (borrowed technique).
- C10 context bandit (SSID, direct/DERP, time) over playout offset + FEC level.
Round 2 plan (after quant + host reports): prior-art agents on C1, C8, C4, C6 (+ best from quant/host). 
Report host-pipeline in. Verified in Sunshine src: KMS capture is TIMER-paced (handle_pacing, avg T/2 wait, worst T); PipeWire/portal can be event-driven; draft Sunshine PR #5538 moves off variable-rate capture. Game render queue (1-2 frames) + fps cap/Reflex is the biggest unmeasured term. NVENC path already near floor (P1, zeroReorder, 1-frame VBV). PipeWire pulse quantum clamps audio to 5.33ms by default. Vulkan Video encode merged in Sunshine 2026.413 (AMD tested only).
- C11 present-aligned / client-clocked capture: GFN Cloud G-Sync (variable stream rate) and Sunshine PR #5538 are near prior art -> likely NOT novel; still a top practical win. 
Report quant in (6/6 DONE). Added candidates: 
- C12 Adaptive Conformal Inference as 1-scalar servo on playout quantile (provable long-run late rate under arbitrary nonstationarity). 
- C13 GPD-hazard optimal-stopping cutoff y*=(L/c-sigma)/xi for wait-vs-conceal. 
- C14 BOCD + Gilbert-Elliott filter sizing FEC a frame ahead + conservative Thompson sampling. 
- C15 pre-committed hedging direct+DERP (needs path-independence test).
ROUND 2 LAUNCHED (3 adversarial prior-art agents): (a) C1 timewarp, (b) C8 + C12 + C13, (c) C6/C15 hedging + C4 psychophysics + software-only photon measurement (C16).
ROUND 2 result (c) in: D hedging PARTIALLY DONE 75% (DMTP draft sec3.4 parity on different path; AUGUR NSDI24 Tencent multipath cloud gaming w/ probabilistic state model; 2022-7/RIST). Browser+tailnet+GE-driven pre-committed hedge unpublished but path independence likely poor (same NIC/uplink) -> test correlation first, 1 day experiment. E personalized QoE PARTIALLY DONE 70% (LingXi SIGCOMM25, Jade, patent US10222952 staircase-on-latency) and identifiability weak. F software-only photon measurement: physically unsound, patents exist. => no wow from (c).
ROUND 2 result (b) in: 
- A PMF+joint MPC: PARTIALLY DONE 70%. Fugu (per-chunk PMF->MPC), Vidaptive (percentile per-frame bitrate), Nebula (bitrate+frame-level redundancy cloud gaming), Boutremans&LeBoudec 2003 (joint rate/error/playout, audio), TAROT, ABRF (unread, biggest threat). Defensible delta = per-frame latency PMF conditioned on FEC level + pacing state, driving the triple. Shield-on-classical = not novel.
- B ACI playout servo: PARTIALLY DONE (Simeone 2025 conformal calibration for wireless latency uses same 1-scalar update; no conformal+jitter-buffer paper found). Publishable as application, low conceptual novelty.
- C GPD hazard stopping: closed form looks unpublished (60%), but formula has a sign/degeneracy issue (xi->0; check max vs min). Framing is old (RaDiO, Liang/Girod).
ROUND 2 result (a) timewarp in: core idea MOSTLY DONE 80%: NVIDIA HPG 2020 (rotation warp + late input recovers 80-90% of aiming penalty at 80-105ms), EA patent US10773162 (overscan bg + camera-dir metadata + separate fg; ACTIVE patent, FTO risk), Reflex2WebFrameWarp GitHub (Three.js, WebRTC, rotation shader, pixel-grid frame ID, custom scene), Reflex 2, cloud-VR timewarp patents, Drizzle (WPI). Unclaimed: (a) UNMODIFIED games -> blocker = generic camera pose/gain.
=> ROUND 3 SEED (the only gap): ONLINE SYSTEM IDENTIFICATION of the input->camera-motion map from decoded video (global-motion/homography from frames or encoder MVs regressed on logged inputs), so client-side rotation warp works on unmodified games with no hooks; self-calibrates gain/deadzone/curve/accel per game; HUD masking by static-region detection. Need: prior art + identifiability/accuracy math vs perceptual thresholds + failure modes.
ROUND 3 prior-art agent in: SHARED WEB SEARCH BUDGET EXHAUSTED (200/session, agent said limit hit after 3 searches). => NO MORE web-search-based prior-art checks this session; only WebFetch of known URLs. Verdict C17 (black-box online identification of input->camera gain + client reprojection): NOVEL as combination ~65% confidence (capped, mostly unsearched). Closest: US10595041 (client input->motion-vector lookup applied to decoded frames, possibly learned/cached; MUST read full claims on Google Patents), Reflex2WebFrameWarp, EA US10773162, Regensburg DL latency compensation (2022), VPT inverse dynamics (video->action). Blockers: aim assist/ADS sensitivity changes, no overscan from unmodified game, encoder MVs not exposed in WebCodecs (must compute flow on decoded frames), parallax/moving objects contaminate fit, hit-reg latency not hidden.
ROUND 3 feasibility in. Sim re-run by me, numbers reproduce (scratchpad/sim-warp/sim.py, plain python). Clean RLS 0.39deg vs 5.05 no-warp (13x) BUT plant is exactly in model family, stick-like input, synthetic noise => overstated. 20% outliers/noise 0.2deg/frame -> 2-3 deg (bad). Aim assist 1.15, ADS switch 1.5 deg. Mouse accel NOT simulated. Needs 5-10 s active input to converge. Gate on confidence.
FINAL: LOOP STOPPED. No verified "wow". Best lead = C17 (black-box online identification + client reprojection), novelty 65% unverified (search budget exhausted), feasibility go-with-caveats. Must-do before claiming anything: read US10595041 claims; real-game offline test (Xonotic/Quake-based, ground-truth angles).
Round 1 complete; no wow yet. After round 2: if nothing survives -> round 3 = wider/weirder brainstorm (max 2 more rounds), else write up winner.
(old waiting line: control/RL, host pipeline, perception, coding/multipath)

## Seeds from me (unresearched)
- client-clocked capture (host captures phase-locked to client vsync)
- hedged duplicate of input+keyframes over direct+DERP via two WebTransport sessions merged at app level
- single-user online-learned controller with classical-controller shield, trained from recorded traces of own path
- render-aware side info from host (depth/motion vectors via Vulkan layer) for client reprojection of flat games
