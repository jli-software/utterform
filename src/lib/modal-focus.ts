// Keep keyboard navigation inside Settings and restore its trigger on close.
export function modalFocus(node: HTMLElement) {
  const previous = document.activeElement as HTMLElement | null;
  const focusable = () => Array.from(node.querySelectorAll<HTMLElement>(
    'button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), [tabindex="0"]',
  )).filter((element) => element.tabIndex >= 0 && !element.closest('[hidden], [inert]'));
  focusable()[0]?.focus();
  function trap(event: KeyboardEvent) {
    if (event.key !== "Tab") return;
    const items = focusable();
    const first = items[0];
    const last = items.at(-1);
    if ((event.shiftKey && document.activeElement === first) || (!event.shiftKey && document.activeElement === last)) {
      event.preventDefault();
      (event.shiftKey ? last : first)?.focus();
    }
  }
  node.addEventListener("keydown", trap, true);
  return { destroy() {
    node.removeEventListener("keydown", trap, true);
    queueMicrotask(() => previous?.focus());
  } };
}
