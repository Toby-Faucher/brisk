<script lang="ts">
  // Web 1.0 trimmings: a ticker on top, a footer with a counter and a webring. All of it is decoration
  // (aria-hidden, pointer-events none); the real controls stay in the sidebar. Not mounted on /stream.
  import { onMount } from 'svelte';
  import { games, watchGames } from '../../state/games';
  import { host } from '../../state/host';
  import { hub, selectedMachine } from '../../state/machines';
  import { STATE_LABEL } from '../../modules/machines/machines';

  let { path }: { path: string } = $props();
  onMount(() => watchGames()); // the footer counter needs the list on every page
  const SUN = { ready: 'Sunshine is ready', busy: 'Someone is streaming', down: 'Sunshine is down' } as const;
  const count = $derived($games?.games.length ?? 0);
  const digits = $derived(String(count).padStart(6, '0').split(''));
  const hostLine = $derived(
    $hub === true
      ? `${$selectedMachine?.name ?? 'nobody'} is ${$selectedMachine ? STATE_LABEL[$selectedMachine.state] : 'not picked yet'}`
      : `${$host?.name ?? 'The host'}: ${$host ? SUN[$host.sunshine] : 'not answering'}`,
  );
  const news = $derived(
    `*** WELCOME TO MY HOME PAGE *** ${hostLine} *** ${count} games in the library *** PLEASE SIGN MY GUESTBOOK *** NEW! IMPROVED! ***`,
  );
</script>

{#if !path.startsWith('/stream')}
  <div class="gc-top" aria-hidden="true">
    <div class="gc-run"><span>{news}</span><span>{news}</span></div>
  </div>
  <div class="gc-bot" aria-hidden="true">
    <span class="gc-best">Best viewed in 1024x768 with Netscape Navigator</span>
    <span class="gc-ring">&lt;&lt; prev | <b>BRISK WEBRING</b> | random | next &gt;&gt;</span>
    <span class="gc-count">
      <span class="gc-lab">Games in the library:</span>
      <span class="gc-lcd">{#each digits as d, i (i)}<i>{d}</i>{/each}</span>
    </span>
    <svg class="gc-mail" viewBox="0 0 32 24" width="30" height="22"
      ><rect x="1" y="3" width="30" height="19" fill="#fff" stroke="#000" stroke-width="2" /><path
        d="M1 4l15 11L31 4"
        fill="none"
        stroke="#000"
        stroke-width="2" /><circle cx="26" cy="5" r="4" fill="#f00" stroke="#000" /></svg
    >
  </div>
{/if}

<style>
  .gc-top,
  .gc-bot {
    position: fixed;
    left: 0;
    right: 0;
    z-index: 30;
    pointer-events: none;
    overflow: hidden;
    display: flex;
    align-items: center;
    background: #000;
    color: #ffff00;
  }
  .gc-top {
    top: 0;
    height: var(--gc-top);
    border-bottom: 3px solid #f0c;
    font-family: var(--display);
    font-size: 13px;
    letter-spacing: 0.04em;
    white-space: nowrap;
  }
  .gc-run {
    display: flex;
    width: max-content;
  }
  .gc-run span {
    padding-right: 4em;
  }
  .gc-bot {
    bottom: 0;
    height: var(--gc-bot);
    gap: 18px;
    padding: 0 12px;
    border-top: 3px solid #0cf;
    font-family: var(--font);
    font-weight: 700;
    font-size: 13px;
    white-space: nowrap;
    color: #fff;
  }
  .gc-best {
    color: #66ffff;
  }
  .gc-ring {
    margin: 0 auto;
    color: #fff;
  }
  .gc-ring b {
    color: #ffff00;
    font-family: var(--display);
    font-weight: 400;
  }
  .gc-count {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .gc-lab {
    color: #ff99ff;
  }
  .gc-lcd {
    display: inline-flex;
    gap: 2px;
    padding: 1px 3px;
    background: #200;
    border: 2px inset #888;
  }
  .gc-lcd i {
    font-style: normal;
    font-family: var(--mono);
    font-size: 22px;
    line-height: 20px;
    width: 14px;
    text-align: center;
    color: #ff3030;
    text-shadow: 0 0 5px #f00;
  }
  .gc-mail {
    flex: none;
  }
  @media (prefers-reduced-motion: no-preference) {
    .gc-run {
      animation: gc-scroll 28s linear infinite;
    }
    .gc-mail {
      transform-origin: 50% 90%;
      animation: gc-wobble 1.6s ease-in-out infinite;
    }
    @keyframes gc-scroll {
      to {
        transform: translateX(-50%);
      }
    }
    @keyframes gc-wobble {
      0%,
      60%,
      100% {
        transform: rotate(0);
      }
      70% {
        transform: rotate(-14deg);
      }
      80% {
        transform: rotate(12deg);
      }
      90% {
        transform: rotate(-8deg);
      }
    }
  }
  @media (max-width: 1100px) {
    .gc-best {
      display: none;
    }
  }
  @media (max-width: 800px) {
    .gc-ring {
      display: none;
    }
  }
</style>
