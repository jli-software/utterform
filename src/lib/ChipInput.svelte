<script lang="ts">
  /// A short list edited one entry at a time: Enter or a comma adds what was
  /// typed, × removes an entry, and a pasted list keeps its lines. Every change
  /// is handed out at once; there is nothing to confirm.
  export let id: string;
  export let label: string;
  export let values: string[];
  export let placeholder = "Add";
  /// Turns a typed entry into the stored one, e.g. lower case for a language.
  export let normalize: (value: string) => string = (value) => value.trim();
  /// Why an entry cannot be stored, or "" when it can. A refused entry stays
  /// in the field with the reason beside it and is never handed out.
  export let validate: (value: string) => string = () => "";
  /// Entries already stored that would be refused now are marked, not hidden.
  export let flagged: (value: string) => boolean = () => false;
  export let onchange: (values: string[]) => void = () => {};

  let draft = "";
  let error = "";

  function add(text = draft) {
    const entries = text.split(/[\n,]/).map(normalize).filter(Boolean);
    if (!entries.length) {
      draft = "";
      error = "";
      return;
    }
    const refused = entries.map(validate).find(Boolean);
    if (refused) {
      draft = text;
      error = refused;
      return;
    }
    draft = "";
    error = "";
    const next = [...new Set([...values, ...entries])];
    if (next.length !== values.length) onchange(next);
  }

  function remove(value: string) {
    onchange(values.filter((other) => other !== value));
  }

  function paste(event: ClipboardEvent) {
    const text = event.clipboardData?.getData("text") ?? "";
    if (!/[\n,]/.test(text)) return;
    event.preventDefault();
    add(`${draft}${text}`);
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Enter" || event.key === ",") {
      event.preventDefault();
      add();
    } else if (event.key === "Backspace" && draft === "" && values.length) {
      remove(values[values.length - 1]);
    }
  }
</script>

<div class="chip-field">
  <div class="chips" role="group" aria-label={label}>
    {#each values as value}
      <span class="chip" class:flagged={flagged(value)}>{value}<button type="button" aria-label={`Remove ${value}`} onclick={() => remove(value)}>×</button></span>
    {/each}
    <input {id} class="chip-input" aria-label={label} aria-invalid={error ? "true" : undefined}
      {placeholder} value={draft} oninput={(event) => { draft = event.currentTarget.value; error = ""; }}
      onkeydown={keydown} onblur={() => add()} onpaste={paste} />
  </div>
  {#if error}<p class="setting-error" role="alert">{error}</p>{/if}
</div>
