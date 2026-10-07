"""Évaluation de l'assistant : compréhension, respect des contraintes, hallucinations, latence.

Lancer l'API avec la configuration LLM à tester (cloud, Ollama, ou aucun LLM), puis :
    python eval/run_eval.py --label ollama-mistral
Le résultat est ajouté à eval/results.csv pour comparer les configurations.
"""

from __future__ import annotations

import argparse
import csv
import json
import time
from pathlib import Path

import requests

HERE = Path(__file__).resolve().parent


# Gare donnée quand l'appli demande « De quelle gare partez-vous ? » (comme le ferait l'utilisateur)
ANSWER_ORIGIN = "Grenoble"


def chat(api: str, message: str, session_id: str | None = None, origin: str | None = None) -> dict:
    body: dict = {"message": message, "session_id": session_id}
    if origin:
        body["context"] = {"origin": origin, "themes": [], "keywords": []}
    r = requests.post(f"{api}/api/chat", json=body, timeout=180)
    r.raise_for_status()
    return r.json()


def chat_answering_origin(api: str, message: str, session_id: str | None = None) -> tuple[dict, bool]:
    """Envoie la demande ; si l'appli demande la gare de départ (non citée), la donne et renvoie la demande."""
    resp = chat(api, message, session_id)
    if resp.get("needs_origin") and not resp["criteria"].get("origin"):
        return chat(api, message, resp["session_id"], ANSWER_ORIGIN), True
    return resp, False


def criteria_accuracy(expected: dict, got: dict) -> float:
    """Part des critères attendus correctement extraits."""
    ok = 0
    for key, value in expected.items():
        g = got.get(key)
        if isinstance(value, list):
            ok += set(value) <= set(g or [])
        elif isinstance(value, str):
            ok += isinstance(g, str) and g.lower() == value.lower()
        else:
            ok += g == value
    return ok / len(expected) if expected else 1.0


def constraints_respected(resp: dict) -> bool:
    """Chaque recommandation respecte-t-elle les contraintes appliquées ?"""
    max_t, max_w = resp["applied_max_travel_minutes"], resp["applied_max_walk_minutes"]
    for r in resp["recommendations"]:
        if r["travel_minutes"] is not None and r["travel_minutes"] > max_t:
            return False
        if any(p["walk_minutes"] > max_w for p in r["pois"]):
            return False
    return True


def hallucinations(resp: dict) -> int:
    """Items de réponse qui ne correspondent à aucune recommandation issue des données."""
    ids = {r["station"]["id"] for r in resp["recommendations"]}
    return sum(1 for i in resp["answer"]["items"] if i["station_id"] not in ids)


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--api", default="http://localhost:8080")
    p.add_argument("--label", required=True, help="nom de la configuration testée (ex. mistral-api, ollama-llama3)")
    a = p.parse_args()

    queries = json.loads((HERE / "queries.json").read_text())
    rows = []
    for q in queries:
        t0 = time.perf_counter()
        resp, asked = chat_answering_origin(a.api, q["message"])
        if q.get("followup"):
            resp = chat(a.api, q["followup"], resp["session_id"])
        latency = int((time.perf_counter() - t0) * 1000)
        n = len(resp["recommendations"])
        row = {
            "label": a.label,
            "query": q["id"],
            "extraction": resp["engine"]["extraction"],
            "generation": resp["engine"]["generation"],
            "criteria_acc": round(criteria_accuracy(q["expected"], resp["criteria"]), 2),
            "constraints_ok": constraints_respected(resp),
            "nb_results": n,
            "no_result_ok": (n == 0) == q.get("expect_no_result", False),
            "hallucinated_items": hallucinations(resp),
            "prompt_tokens": resp["engine"]["usage"]["prompt_tokens"],
            "completion_tokens": resp["engine"]["usage"]["completion_tokens"],
            "latency_ms": latency,
            "relevance_1_5": "",  # à noter à la main en relisant les réponses
            "origin_asked": asked,  # gare de départ non citée : l'appli l'a demandée
        }
        rows.append(row)
        print(f"{q['id']:<18} critères={row['criteria_acc']:.2f} contraintes={row['constraints_ok']} "
              f"résultats={n} halluc={row['hallucinated_items']} {latency} ms")

    out = HERE / "results.csv"
    if out.exists() and out.read_text().split("\n", 1)[0] != ",".join(rows[0].keys()):
        out.rename(HERE / "results_ancien_format.csv")  # colonnes changées : on garde l'ancien fichier à part
    new = not out.exists()
    with out.open("a", newline="") as f:
        w = csv.DictWriter(f, fieldnames=rows[0].keys())
        if new:
            w.writeheader()
        w.writerows(rows)

    n = len(rows)
    print(f"\n{a.label} : compréhension moyenne {sum(r['criteria_acc'] for r in rows) / n:.2f}, "
          f"contraintes respectées {sum(r['constraints_ok'] for r in rows)}/{n}, "
          f"gare demandée {sum(r['origin_asked'] for r in rows)}/{n}, "
          f"latence moyenne {sum(r['latency_ms'] for r in rows) // n} ms, "
          f"tokens moyens {sum(r['prompt_tokens'] for r in rows) // n} in / {sum(r['completion_tokens'] for r in rows) // n} out")
    print(f"Résultats ajoutés à {out}")


if __name__ == "__main__":
    main()
