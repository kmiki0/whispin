// Tiny DOM + UX helpers shared across all settings sections.

export const $ = <T extends HTMLElement>(sel: string) =>
  document.querySelector(sel) as T;

let statusEl: HTMLParagraphElement | null = null;

export function initFlash(el: HTMLParagraphElement) {
  statusEl = el;
}

export function flash(msg: string, isError = false) {
  if (!statusEl) return;
  statusEl.textContent = msg;
  statusEl.classList.toggle("error", isError);
  window.setTimeout(() => {
    if (statusEl) statusEl.textContent = "";
  }, 2200);
}

/// Wire up sidebar nav: clicking a `.nav-item[data-section]` shows the
/// matching `.section[data-section-content]`.
export function installSectionNav() {
  document.querySelectorAll<HTMLButtonElement>(".nav-item").forEach((btn) => {
    btn.addEventListener("click", () => {
      const section = btn.dataset.section;
      document
        .querySelectorAll(".nav-item")
        .forEach((el) => el.classList.toggle("active", el === btn));
      document
        .querySelectorAll<HTMLElement>(".section")
        .forEach((el) =>
          el.classList.toggle("active", el.dataset.sectionContent === section),
        );
    });
  });
}
