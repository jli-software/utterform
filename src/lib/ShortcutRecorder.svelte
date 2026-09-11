<script lang="ts">
  import { onDestroy } from "svelte";
  import { formatShortcut, readShortcut } from "./shortcut";

  export let id: string;
  export let label: string;
  export let value: string;
  export let apple = false;
  export let onchange: (shortcut: string) => void = () => {};
  /// Told whenever the control starts and stops listening, so the caller can
  /// release a key the operating system is still holding for Utterform — a
  /// registered shortcut never reaches the window it was typed into.
  export let oncapture: (capturing: boolean) => void = () => {};

  let capturing = false;
  let message = "";

  $: display = formatShortcut(value, apple);
  $: status = message || (capturing ? "Listening for a shortcut. Press Escape to keep the current one." : "");

  function start() {
    if (capturing) return;
    message = "";
    capturing = true;
    // In the capture phase on the window: the combination being recorded may
    // be one the dialog, the focus trap or the main window would act on, and
    // while a shortcut is being read it belongs to nobody else.
    window.addEventListener("keydown", read, true);
    oncapture(true);
  }

  function stop() {
    if (!capturing) return;
    capturing = false;
    window.removeEventListener("keydown", read, true);
    oncapture(false);
  }

  function read(event: KeyboardEvent) {
    event.preventDefault();
    event.stopPropagation();
    // Escape ends the reading and nothing else: the dialog stays open, and the
    // shortcut that was there before is still the one in effect.
    if (event.key === "Escape") {
      message = "";
      stop();
      return;
    }
    const reading = readShortcut(event, apple);
    if (reading.status === "captured") {
      message = "";
      stop();
      if (reading.shortcut !== value) onchange(reading.shortcut);
      return;
    }
    // A modifier on its own waits for the key it belongs to; a key on its own
    // says what is missing and keeps listening, so the next press can fix it.
    if (reading.status === "unmodified") message = reading.message;
  }

  onDestroy(stop);
</script>

<div class="field shortcut-field">
  <span id={`${id}-label`}>{label} <small>{capturing ? "press the keys you want" : "click, then press the keys you want"}</small></span>
  <button type="button" class="shortcut-recorder" class:capturing id={id}
    aria-labelledby={`${id}-label ${id}-value`} aria-pressed={capturing}
    onclick={() => (capturing ? stop() : start())} onblur={stop}>
    <span id={`${id}-value`} class="shortcut-value">{capturing ? "Press shortcut…" : display}</span>
    <kbd>{capturing ? "Esc" : "Change"}</kbd>
  </button>
  <span class="visually-hidden" role="status">{status}</span>
  {#if message}<p class="setting-error">{message}</p>{/if}
</div>
