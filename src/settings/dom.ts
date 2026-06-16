// Tiny DOM + UX helpers shared across all settings sections.

export const $ = <T extends HTMLElement>(sel: string) =>
  document.querySelector(sel) as T;

let statusEl: HTMLParagraphElement | null = null;
let flashTimer = 0;

export function initFlash(el: HTMLParagraphElement) {
  statusEl = el;
}

export function flash(msg: string, isError = false) {
  if (!statusEl) return;
  statusEl.textContent = msg;
  statusEl.classList.toggle("error", isError);
  // Clear any pending timer so a previous flash doesn't wipe this message early.
  if (flashTimer) window.clearTimeout(flashTimer);
  flashTimer = window.setTimeout(() => {
    if (statusEl) statusEl.textContent = "";
    flashTimer = 0;
  }, 2200);
}

/// Wire up sidebar nav: clicking a `.nav-item[data-section]` shows the
/// matching `.section[data-section-content]`. Also supports up/down/home/end
/// keyboard navigation and exposes the selection state via aria.
export function installSectionNav() {
  const items = Array.from(
    document.querySelectorAll<HTMLButtonElement>(".nav-item"),
  );

  function select(btn: HTMLButtonElement) {
    const section = btn.dataset.section;
    for (const el of items) {
      const active = el === btn;
      el.classList.toggle("active", active);
      el.setAttribute("aria-selected", active ? "true" : "false");
      el.tabIndex = active ? 0 : -1;
    }
    document
      .querySelectorAll<HTMLElement>(".section")
      .forEach((el) =>
        el.classList.toggle("active", el.dataset.sectionContent === section),
      );
    btn.focus();
  }

  items.forEach((btn, i) => {
    btn.setAttribute("role", "tab");
    btn.setAttribute(
      "aria-selected",
      btn.classList.contains("active") ? "true" : "false",
    );
    btn.tabIndex = btn.classList.contains("active") ? 0 : -1;

    btn.addEventListener("click", () => select(btn));
    btn.addEventListener("keydown", (e) => {
      let next = -1;
      if (e.key === "ArrowDown") next = (i + 1) % items.length;
      else if (e.key === "ArrowUp") next = (i - 1 + items.length) % items.length;
      else if (e.key === "Home") next = 0;
      else if (e.key === "End") next = items.length - 1;
      if (next >= 0) {
        e.preventDefault();
        select(items[next]);
      }
    });
  });

  const list = items[0]?.closest("ul");
  list?.setAttribute("role", "tablist");
}
