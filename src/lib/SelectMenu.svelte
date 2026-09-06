<script lang="ts">
  import { tick } from "svelte";

  export let id: string;
  export let label: string;
  export let value: string;
  export let options: Array<{ value: string; label: string; hint?: string; key?: string }>;
  export let disabled = false;
  export let compact = false;
  export let upwards = false;
  export let onchange: (value: string) => void = () => {};

  let expanded = false;
  let activeIndex = 0;
  let root: HTMLDivElement;
  let trigger: HTMLButtonElement;
  let search = "";
  let searchedAt = 0;

  $: selected = options.find((option) => option.value === value);
  $: if (disabled) expanded = false;

  async function reveal() {
    if (disabled || !options.length) return;
    activeIndex = Math.max(0, options.findIndex((option) => option.value === value));
    expanded = true;
    await scrollActive();
  }

  async function scrollActive() {
    await tick();
    document.getElementById(`${id}-option-${activeIndex}`)?.scrollIntoView?.({ block: "nearest" });
  }

  function choose(index: number) {
    if (!options[index]) return;
    value = options[index].value;
    expanded = false;
    trigger.focus();
    onchange(value);
  }

  function keydown(event: KeyboardEvent) {
    // App copy-again shortcuts also work while the selector owns focus.
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    // A closed selector lets Escape reach the containing dialog.
    if (event.key === "Escape" && !expanded) return;
    // Menu navigation must not also trigger recording/action shortcuts.
    event.stopPropagation();
    if (event.key === "Tab") { expanded = false; return; }
    if (event.key === "Escape") {
      if (expanded) { event.preventDefault(); expanded = false; trigger.focus(); }
      return;
    }
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      if (!expanded) { void reveal(); return; }
      if (event.key === "Home") activeIndex = 0;
      else if (event.key === "End") activeIndex = options.length - 1;
      else activeIndex = (activeIndex + (event.key === "ArrowDown" ? 1 : -1) + options.length) % options.length;
      void scrollActive();
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      if (expanded) choose(activeIndex);
      else void reveal();
    } else if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault();
      search = Date.now() - searchedAt > 700 ? event.key : search + event.key;
      searchedAt = Date.now();
      const index = options.findIndex((option) => option.label.toLowerCase().startsWith(search.toLowerCase()));
      if (index >= 0) {
        expanded = true;
        activeIndex = index;
        void scrollActive();
      }
    }
  }
</script>

<svelte:window onpointerdown={(event) => { if (root && !root.contains(event.target as Node)) expanded = false; }} />

<div class="select-menu" class:compact class:upwards bind:this={root}
  onfocusout={(event) => { if (!root.contains(event.relatedTarget as Node | null)) expanded = false; }}>
  <button type="button" class="select-trigger" bind:this={trigger} role="combobox"
    aria-label={label} aria-haspopup="listbox" aria-expanded={expanded} aria-controls={`${id}-list`}
    aria-activedescendant={expanded ? `${id}-option-${activeIndex}` : undefined}
    {disabled} onkeydown={keydown} onclick={() => expanded ? expanded = false : void reveal()}>
    <span class="select-value"><strong>{selected?.label ?? label}</strong>{#if selected?.hint && !compact}<small>{selected.hint}</small>{/if}</span>
    <svg viewBox="0 0 20 20" aria-hidden="true" class:expanded><path d="m5 7.5 5 5 5-5" /></svg>
  </button>
  {#if expanded}
    <div id={`${id}-list`} class="select-options" role="listbox" aria-label={label}>
      {#each options as option, index}
        <button type="button" id={`${id}-option-${index}`} role="option" tabindex="-1"
          aria-selected={option.value === value} class:highlighted={index === activeIndex}
          onpointermove={() => activeIndex = index} onkeydown={keydown} onclick={() => choose(index)}>
          <span><strong>{option.label}</strong>{#if option.hint}<small>{option.hint}</small>{/if}</span>
          {#if option.value === value}<span class="select-check" aria-hidden="true">✓</span>{:else if option.key}<kbd>{option.key}</kbd>{/if}
        </button>
      {/each}
    </div>
  {/if}
</div>
