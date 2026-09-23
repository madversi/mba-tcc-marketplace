import argparse
from pathlib import Path

import numpy as np
import pandas as pd
from scipy import stats

SEMENTE = 20260917
B = 10000

METRICAS = {
    "1A": [("medicao", m) for m in ["lat_p50_ms", "lat_p95_ms", "saga_p50_ms", "saga_p95_ms", "vazao_efetiva_rps",
                                     "cpu_media_orders", "cpu_media_payments", "mem_max_mb_orders", "mem_max_mb_payments"]],
    "1B": [("medicao", m) for m in ["lat_p50_ms", "lat_p95_ms", "lat_p99_ms", "vazao_efetiva_rps", "erro_pct",
                                     "saga_p50_ms", "saga_p95_ms"]
           + [f"cpu_media_{s}" for s in ["postgres", "rabbitmq", "toxiproxy", "catalog", "orders", "inventory", "payments"]]
           + [f"mem_max_mb_{s}" for s in ["postgres", "rabbitmq", "toxiproxy", "catalog", "orders", "inventory", "payments"]]],
    "2A": [("falha", "erro_pct"), ("falha", "lat_p95_ms"), ("falha", "fator_amplificacao"), ("falha", "fallback_pct_201"),
           ("execucao", "cb_catalogo_primeira_abertura_s"), ("execucao", "recuperacao_s")],
    "2B": [("falha", "erro_pct"), ("falha", "lat_p95_ms"), ("falha", "fator_amplificacao"), ("falha", "fallback_pct_201"),
           ("execucao", "cb_catalogo_primeira_abertura_s"), ("execucao", "recuperacao_s")],
    "3A": [("antes", "confirmados_pct"), ("falha", "confirmados_pct"), ("depois", "confirmados_pct"),
           ("execucao", "msg_reentregues"), ("execucao", "msg_mortas"), ("execucao", "reprocessamento_p50_s"),
           ("execucao", "reprocessamento_max_s"), ("execucao", "drenagem_segundos")],
    "4A": [("antes", "confirmados_pct"), ("falha", "confirmados_pct"), ("depois", "confirmados_pct"),
           ("execucao", "msg_reentregues"), ("execucao", "msg_mortas"), ("execucao", "drenagem_segundos")],
}

ADJACENTES = {
    "1A": ["taxa-010", "taxa-020", "taxa-040", "taxa-060"],
    "2A": ["C0", "C1", "C2", "C3", "C4"],
    "2B": ["C0", "C2", "C3", "C4"],
}


def boot_mediana(x, rng):
    idx = rng.integers(0, len(x), size=(B, len(x)))
    return np.median(x[idx], axis=1)


def ic(amostras):
    return np.percentile(amostras, 2.5), np.percentile(amostras, 97.5)


def holm(p):
    p = np.asarray(p, dtype=float)
    ordem = np.argsort(p)
    m = len(p)
    ajustado = np.empty(m)
    acumulado = 0.0
    for posicao, i in enumerate(ordem):
        acumulado = max(acumulado, min(1.0, (m - posicao) * p[i]))
        ajustado[i] = acumulado
    return ajustado


def valores(df, condicao, fase, metrica):
    sel = df[(df["condicao"] == condicao) & (df["fase"] == fase)]
    if metrica not in sel.columns:
        return np.array([])
    return pd.to_numeric(sel[metrica], errors="coerce").dropna().to_numpy()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", default=str(Path(__file__).resolve().parent))
    args = parser.parse_args()
    base = Path(args.base)
    rng = np.random.default_rng(SEMENTE)

    medianas, comparacoes, mw = [], [], []
    for exp, metricas in METRICAS.items():
        arquivo = base / "consolidado" / f"{exp}.csv"
        if not arquivo.exists() or arquivo.stat().st_size == 0:
            continue
        df = pd.read_csv(arquivo, dtype={"valida": str})
        df = df[df["valida"] == "true"]
        condicoes = sorted(df["condicao"].unique())
        for fase, metrica in metricas:
            for c in condicoes:
                x = valores(df, c, fase, metrica)
                linha = {"experimento": exp, "condicao": c, "fase": fase, "metrica": metrica, "n": len(x)}
                if len(x) > 0:
                    lo, hi = ic(boot_mediana(x, rng))
                    linha.update(mediana=np.median(x), q1=np.percentile(x, 25), q3=np.percentile(x, 75),
                                 ic95_inf=lo, ic95_sup=hi)
                medianas.append(linha)

            pares = []
            if exp in ADJACENTES:
                ordem = [c for c in ADJACENTES[exp] if c in condicoes]
                pares = list(zip(ordem[:-1], ordem[1:]))
            elif exp == "1B" and {"C0-ROFF", "C4-RON"} <= set(condicoes):
                pares = [("C0-ROFF", "C4-RON")]
            elif exp == "3A" and {"R-OFF", "R-ON"} <= set(condicoes):
                pares = [("R-OFF", "R-ON")]
            familia = []
            for a, b in pares:
                xa, xb = valores(df, a, fase, metrica), valores(df, b, fase, metrica)
                linha = {"experimento": exp, "fase": fase, "metrica": metrica, "a": a, "b": b,
                         "n_a": len(xa), "n_b": len(xb)}
                if len(xa) > 0 and len(xb) > 0:
                    d = boot_mediana(xb, rng) - boot_mediana(xa, rng)
                    lo, hi = ic(d)
                    p = min(1.0, 2 * min(np.mean(d <= 0), np.mean(d >= 0)))
                    linha.update(mediana_a=np.median(xa), mediana_b=np.median(xb),
                                 diferenca_b_menos_a=np.median(xb) - np.median(xa),
                                 ic95_inf=lo, ic95_sup=hi, p_bootstrap=p)
                familia.append(linha)
            ps = [l.get("p_bootstrap", np.nan) for l in familia]
            validos = [i for i, p in enumerate(ps) if not np.isnan(p)]
            if validos and exp in ADJACENTES:
                ajustados = holm([ps[i] for i in validos])
                for i, pa in zip(validos, ajustados):
                    familia[i]["p_holm"] = pa
            comparacoes.extend(familia)

            if exp == "2A" and {"C2", "C3"} <= set(condicoes):
                x2, x3 = valores(df, "C2", fase, metrica), valores(df, "C3", fase, metrica)
                linha = {"experimento": exp, "fase": fase, "metrica": metrica, "n_C2": len(x2), "n_C3": len(x3)}
                if len(x2) > 0 and len(x3) > 0:
                    res = stats.mannwhitneyu(x3, x2, alternative="two-sided", method="auto")
                    u = res.statistic
                    delta = 2 * u / (len(x2) * len(x3)) - 1
                    linha.update(mediana_C2=np.median(x2), mediana_C3=np.median(x3), U_C3=u, p=res.pvalue,
                                 delta_cliff_C3_vs_C2=delta)
                mw.append(linha)

    saida = base / "tabelas"
    saida.mkdir(parents=True, exist_ok=True)
    pd.DataFrame(medianas).to_csv(saida / "estatistica_medianas.csv", index=False)
    pd.DataFrame(comparacoes).to_csv(saida / "estatistica_comparacoes.csv", index=False)
    pd.DataFrame(mw).to_csv(saida / "estatistica_mannwhitney_2A_C2xC3.csv", index=False)

    t5 = saida / "tabela5.csv"
    if t5.exists() and t5.stat().st_size > 0:
        tab = pd.read_csv(t5)
        comp = pd.DataFrame(comparacoes)
        comp = comp[(comp["experimento"] == "1B")] if len(comp) else comp
        for i, linha in tab.iterrows():
            achado = comp[comp["metrica"] == linha["metrica"]] if len(comp) else comp
            if len(achado) == 1 and "ic95_inf" in achado:
                tab.loc[i, "ic95_inf"] = achado["ic95_inf"].iloc[0]
                tab.loc[i, "ic95_sup"] = achado["ic95_sup"].iloc[0]
        tab.to_csv(t5, index=False)

    print(f"medianas: {len(medianas)} linhas; comparacoes: {len(comparacoes)}; mann-whitney: {len(mw)}")


if __name__ == "__main__":
    main()
