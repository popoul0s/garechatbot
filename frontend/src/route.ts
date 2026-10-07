// Navigation minimale par l'ancre d'URL : "#/" = accueil, "#/app" = appli, "#/app?q=..." = appli + demande.

export type Page = "home" | "app";

export function current(): { page: Page; q: string | null } {
  const h = window.location.hash;
  if (!h.startsWith("#/app")) return { page: "home", q: null };
  const q = new URLSearchParams(h.split("?")[1] ?? "").get("q");
  return { page: "app", q: q && q.trim() ? q : null };
}

export function go(page: Page, q?: string) {
  window.location.hash = page === "home" ? "#/" : q ? `#/app?q=${encodeURIComponent(q)}` : "#/app";
  window.scrollTo(0, 0);
}
