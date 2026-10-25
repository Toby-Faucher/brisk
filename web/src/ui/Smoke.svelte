<script lang="ts">
  // Storm Flow smoke in the accent colour. Off (CSS gradient, no WebGL context) on /stream, without WebGL2 or
  // float targets, or when settings.shaders is false. The context is destroyed on unmount.
  import { onMount } from 'svelte';
  import { settings } from '../state/settings';
  import { runSmoke, smokeAllowed, smokeGL, type SmokeRun } from './smoke';

  let { accent, interactive = false, active = true }: { accent: string; interactive?: boolean; active?: boolean } = $props();
  let cv: HTMLCanvasElement | undefined = $state();
  let run: SmokeRun | null = $state(null);

  onMount(() => {
    // settings.shaders off -> remount happens on the next navigation; good enough for a settings toggle
    if (!cv || !smokeAllowed(location.pathname, $settings.shaders)) return;
    const gl = smokeGL<WebGL2RenderingContext>(cv);
    run = gl && runSmoke(cv, gl, () => accent, interactive);
    if (!run) (gl?.getExtension('WEBGL_lose_context') as WEBGL_lose_context | null)?.loseContext();
    return () => run?.destroy();
  });
  $effect(() => run?.setActive(active));
</script>

{#if !run}<div class="fallback" style:--c={accent}></div>{/if}
<canvas bind:this={cv} hidden={!run} aria-hidden="true"></canvas>

<style>
  canvas, .fallback { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; }
  .fallback { background: radial-gradient(ellipse at 70% 110%, color-mix(in srgb, var(--c) 45%, transparent), transparent 70%); }
</style>
