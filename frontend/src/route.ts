// Navigation minimale par l'ancre d'URL : "#/" = accueil, "#/app" = appli, "#/app?q=..." = appli + demande.
import { flushSync } from "react-dom";

export type Page = "home" | "app";
export type Route = { page: Page; q: string | null };

export function current(): Route {
  const h = window.location.hash;
  if (!h.startsWith("#/app")) return { page: "home", q: null };
  const q = new URLSearchParams(h.split("?")[1] ?? "").get("q");
  return { page: "app", q: q && q.trim() ? q : null };
}

let render: ((r: Route) => void) | null = null;
/** Branché par la racine : applique une route de façon synchrone (nécessaire aux transitions de vue). */
export function onNavigate(fn: (r: Route) => void) {
  render = fn;
}

type ViewTransitionDoc = Document & { startViewTransition?: (cb: () => void) => unknown };

export function go(page: Page, q?: string) {
  const hash = page === "home" ? "#/" : q ? `#/app?q=${encodeURIComponent(q)}` : "#/app";
  const apply = () => {
    history.pushState(null, "", hash);
    window.scrollTo(0, 0);
    const r = current();
    if (render) flushSync(() => render!(r));
  };
  const doc = document as ViewTransitionDoc;
  const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  // le champ de recherche, le logo et le réseau se transforment en leur équivalent dans l'appli
  if (doc.startViewTransition && !reduced) {
    document.documentElement.dataset.nav = page === "app" ? "forward" : "back";
    doc.startViewTransition(apply);
  } else {
    // navigateur sans transitions de vue : simple fondu d'entrée (voir landing.css)
    document.documentElement.dataset.nav = reduced ? "" : "none";
    apply();
  }
}
