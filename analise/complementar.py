import argparse
import itertools
from pathlib import Path

import numpy as np
import pandas as pd
from scipy import stats

from estatistica import B, SEMENTE, boot_mediana, ic

TERMINAIS = ["CONFIRMED", "CANCELLED"]
MEDICAO_S = 180
JANELA_S = 5


def carregar(base, exp):
    df = pd.read_csv(base / "consolidado" / f"{exp}.csv", dtype={"valida": str})
    return df[df["valida"] == "true"]


def valores(df, condicao, fase, metrica):
    sel = df[(df["condicao"] == condicao) & (df["fase"] == fase)].sort_values("repeticao")
    return pd.to_numeric(sel[metrica], errors="coerce").dropna().to_numpy()


def pendentes(pedidos, t):
    terminal = pedidos["status"].isin(TERMINAIS)
    return int(((pedidos["criado"] < t) & (~terminal | (pedidos["atualizado"] >= t))).sum())


def fila_e_sagas_1A(base, resultados):
    df = carregar(base, "1A")
    janelas = pd.read_csv(base / "janelas" / "1A.csv", dtype={"valida": str})
    linhas = []
    for _, m in df[df["fase"] == "medicao"].sort_values(["condicao", "repeticao"]).iterrows():
        rep = int(m["repeticao"])
        pedidos = pd.read_csv(resultados / "1A" / m["condicao"] / f"rep-{rep:02d}" / "sql" / "pedidos.csv")
        pedidos["criado"] = pd.to_datetime(pedidos["created_at"], format="ISO8601", utc=True)
        pedidos["atualizado"] = pd.to_datetime(pedidos["updated_at"], format="ISO8601", utc=True)
        ini = pd.Timestamp(m["fase_ini_utc"])
        fim = ini + pd.Timedelta(seconds=MEDICAO_S)
        terminal = pedidos["status"].isin(TERMINAIS)
        criados = int(((pedidos["criado"] >= ini) & (pedidos["criado"] < fim)).sum())
        concluidos = int((terminal & (pedidos["atualizado"] >= ini) & (pedidos["atualizado"] < fim)).sum())
        j = janelas[(janelas["valida"] == "true") & (janelas["condicao"] == m["condicao"])
                    & (janelas["repeticao"] == rep) & (janelas["t_rel_s"] >= 0)
                    & (janelas["t_rel_s"] < MEDICAO_S)].sort_values("t_rel_s")
        inclinacao = np.polyfit(j["t_rel_s"].to_numpy() + JANELA_S / 2, j["fila_media"].to_numpy(), 1)[0]
        linhas.append({
            "condicao": m["condicao"], "repeticao": rep,
            "fila_media_3_primeiras": m["fila_media_3_primeiras"], "fila_media_3_ultimas": m["fila_media_3_ultimas"],
            "inclinacao_fila_msg_s": inclinacao,
            "pedidos_criados_medicao": criados, "sagas_concluidas_medicao": concluidos,
            "conclusao_sagas_s": concluidos / MEDICAO_S, "razao_conclusao_chegada": concluidos / criados,
            "pendentes_inicio_medicao": pendentes(pedidos, ini), "pendentes_fim_medicao": pendentes(pedidos, fim),
        })
    return pd.DataFrame(linhas)


def mann_whitney_minimo(n_a, n_b):
    return 2 / len(list(itertools.combinations(range(n_a + n_b), n_a)))


def sensibilidade(base, rng):
    comparacoes = [
        ("1A", "medicao", "lat_p50_ms", "taxa-020", "taxa-040"),
        ("1A", "medicao", "saga_p50_ms", "taxa-020", "taxa-040"),
        ("1A", "medicao", "saga_p50_ms", "taxa-040", "taxa-060"),
        ("1A", "medicao", "saga_p95_ms", "taxa-020", "taxa-040"),
        ("1A", "medicao", "saga_p95_ms", "taxa-040", "taxa-060"),
        ("1B", "medicao", "lat_p50_ms", "C0-ROFF", "C4-RON"),
        ("1B", "medicao", "lat_p95_ms", "C0-ROFF", "C4-RON"),
        ("1B", "medicao", "cpu_media_orders", "C0-ROFF", "C4-RON"),
        ("1B", "medicao", "cpu_media_payments", "C0-ROFF", "C4-RON"),
    ]
    linhas = []
    for exp, fase, metrica, a, b in comparacoes:
        df = carregar(base, exp)
        xa, xb = valores(df, a, fase, metrica), valores(df, b, fase, metrica)
        d = boot_mediana(xb, rng) - boot_mediana(xa, rng)
        lo, hi = ic(d)
        welch = stats.ttest_ind(xb, xa, equal_var=False)
        ic_welch = welch.confidence_interval(0.95)
        pares = np.subtract.outer(xb, xa)
        linhas.append({
            "experimento": exp, "fase": fase, "metrica": metrica, "a": a, "b": b,
            "valores_a": ";".join(f"{v:.6g}" for v in xa), "valores_b": ";".join(f"{v:.6g}" for v in xb),
            "diferenca_medianas": np.median(xb) - np.median(xa), "ic95_boot_inf": lo, "ic95_boot_sup": hi,
            "diferenca_pares_min": pares.min(), "diferenca_pares_max": pares.max(),
            "diferenca_medias": xb.mean() - xa.mean(), "ic95_welch_inf": ic_welch.low, "ic95_welch_sup": ic_welch.high,
            "p_welch": welch.pvalue,
            "p_mann_whitney_exato": stats.mannwhitneyu(xb, xa, alternative="two-sided", method="exact").pvalue,
            "p_mann_whitney_minimo_possivel": mann_whitney_minimo(len(xa), len(xb)),
        })
    return pd.DataFrame(linhas)


def pontos_centrais(base):
    linhas = []
    for exp, fase, metricas, condicoes in [
        ("1B", "medicao", ["lat_p95_ms", "cpu_media_orders"], ["C0-ROFF", "C4-RON"]),
        ("2A", "falha", ["lat_p95_ms", "fator_amplificacao", "erro_pct"], ["C2", "C3"]),
        ("3A", "execucao", ["reprocessamento_p50_s", "reprocessamento_max_s"], ["R-ON"]),
    ]:
        df = carregar(base, exp)
        for c in condicoes:
            sel = df[(df["condicao"] == c) & (df["fase"] == fase)].sort_values("repeticao")
            for _, r in sel.iterrows():
                for metrica in metricas:
                    linhas.append({"experimento": exp, "condicao": c, "fase": fase, "repeticao": r["repeticao"],
                                   "metrica": metrica, "valor": r[metrica]})
    return pd.DataFrame(linhas)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", default=str(Path(__file__).resolve().parent))
    parser.add_argument("--resultados", default=None)
    args = parser.parse_args()
    base = Path(args.base)
    resultados = Path(args.resultados) if args.resultados else base.parent / "load-tests" / "results" / "experimentos"
    rng = np.random.default_rng(SEMENTE)

    saida = base / "tabelas"
    saida.mkdir(parents=True, exist_ok=True)
    sens = sensibilidade(base, rng)
    pontos = pontos_centrais(base)
    sens.to_csv(saida / "complementar_sensibilidade.csv", index=False)
    pontos.to_csv(saida / "complementar_pontos_por_execucao.csv", index=False)
    print(f"sensibilidade: {len(sens)} linhas; pontos: {len(pontos)} (B={B})")
    # a parte de fila e sagas do 1A le os pedidos.csv brutos de cada execucao, que nao sao publicados
    if (resultados / "1A").is_dir():
        fila = fila_e_sagas_1A(base, resultados)
        fila.to_csv(saida / "complementar_1A_por_execucao.csv", index=False)
        print(f"1A por execucao: {len(fila)} linhas")
    else:
        print(f"1A por execucao: mantido o CSV publicado ({resultados / '1A'} nao encontrado)")


if __name__ == "__main__":
    main()
