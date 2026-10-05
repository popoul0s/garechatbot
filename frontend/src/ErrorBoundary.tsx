import { Component, ReactNode } from "react";

/** Affiche l'erreur au lieu d'une page blanche si un composant plante. */
export default class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    if (this.state.error) {
      return (
        <div style={{ padding: 24, fontFamily: "system-ui" }}>
          <h2>Une erreur est survenue</h2>
          <pre style={{ whiteSpace: "pre-wrap", color: "#991b1b" }}>{this.state.error.message}</pre>
          <button onClick={() => location.reload()}>Recharger</button>
        </div>
      );
    }
    return this.props.children;
  }
}
