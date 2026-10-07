import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "maplibre-gl/dist/maplibre-gl.css";
import "./styles.css";
import "./landing.css";
import App from "./App";
import ErrorBoundary from "./ErrorBoundary";
import Landing from "./Landing";
import { current } from "./route";

function Root() {
  const [route, setRoute] = useState(current);
  useEffect(() => {
    const onHash = () => setRoute(current());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  // la demande saisie sur l'accueil est passée à l'appli, qui la lance au montage
  return route.page === "home" ? <Landing /> : <App key={route.q ?? ""} initialQuery={route.q} />;
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ErrorBoundary>
      <Root />
    </ErrorBoundary>
  </StrictMode>,
);
