<script lang="ts" module>
  import { api, type Game } from '../state/api';
  import { refreshGames } from '../state/games';
  import { quitFlow } from '../state/quit';
  import { toast } from './Toasts.svelte';

  // One flow for the page: Library, Machines and the command menu share its busy set.
  const flow = quitFlow({ quit: api.quit, refresh: refreshGames, toast });
  /** Appids being quit now: their Quit buttons are aria-disabled. */
  export const quitting = flow.busy;
  /** Appids a quit left running: Ctrl+K offers Force quit. */
  export const stuck = flow.stuck;
  let back: HTMLElement | null = null;

  /** Opens the confirm; focus returns to what had it when the confirm closes. */
  export function askQuit(g: Game, force = false) {
    back = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    flow.request(g, force);
  }
</script>

<script lang="ts">
  import { Dialog } from 'bits-ui';
  const ask = flow.ask;
  const verb = $derived($ask?.force ? 'Force quit' : 'Quit');
</script>

<Dialog.Root bind:open={() => !!$ask, (v) => !v && flow.cancel()}>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim" />
    <Dialog.Content
      class="rs glass"
      onCloseAutoFocus={(e) => {
        if (back?.isConnected) {
          e.preventDefault();
          back.focus();
        }
      }}
    >
      {#if $ask}
        <Dialog.Title class="rs-t">{verb} {$ask.game.name}?</Dialog.Title>
        <Dialog.Description class="lab">{$ask.force ? 'It gets 3 seconds to close, then it is stopped. Unsaved progress will be lost.' : 'Unsaved progress may be lost.'}</Dialog.Description>
        <div class="rs-a">
          <Dialog.Close class="btn ghost">Cancel</Dialog.Close>
          <button class="btn" onclick={() => void flow.confirm()}>{verb}</button>
        </div>
      {/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  /* Confirm dialogs (this one and Machines' restart). Mounted on every page, so global here. */
  :global(.rs) { position: fixed; left: 50%; top: 30%; transform: translateX(-50%); width: min(380px, calc(100vw - 32px)); padding: 20px; z-index: 41; display: grid; gap: 6px; }
  :global(.rs-t) { font-size: 16px; font-weight: 650; }
  :global(.rs-a) { display: flex; gap: 8px; justify-content: flex-end; margin-top: 14px; }
</style>
