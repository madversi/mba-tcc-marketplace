import argparse
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd

PRETO = "#000000"
CORES = ["#000000", "#E69F00", "#56B4E9", "#009E73", "#D55E00", "#CC79A7", "#0072B2"]
ESTILOS = ["-", "--", "-.", ":", (0, (5, 1)), (0, (3, 1, 1, 1, 1, 1)), (0, (1, 1))]

plt.rcParams.update({
    "font.family": "Arial",
    "font.size": 10,
    "axes.titlesize": 10,
    "axes.labelsize": 10,
    "xtick.labelsize": 9,
    "ytick.labelsize": 9,
    "legend.fontsize": 9,
    "text.color": PRETO,
    "axes.labelcolor": PRETO,
    "xtick.color": PRETO,
    "ytick.color": PRETO,
    "axes.edgecolor": PRETO,
    "axes.linewidth": 1.5,
    "xtick.major.width": 1.5,
    "ytick.major.width": 1.5,
    "axes.grid": False,
    "axes.facecolor": "none",
    "figure.facecolor": "none",
    "savefig.facecolor": "none",
    "legend.frameon": False,
})


def eixo(ax, letra):
    for lado in ("top", "right"):
        ax.spines[lado].set_visible(False)
    ax.text(-0.14, 1.02, letra, transform=ax.transAxes, fontsize=11, fontweight="bold", va="bottom", ha="left")


def salvar(fig, destino):
    fig.tight_layout()
    fig.savefig(destino.with_suffix(".png"), dpi=300, transparent=True)
    fig.savefig(destino.with_suffix(".pdf"), transparent=True)
    plt.close(fig)


def ler(caminho):
    if not caminho.exists() or caminho.stat().st_size == 0:
        return None
    return pd.read_csv(caminho)


def figura2(base, saida):
    df = ler(base / "figuras" / "figura2.csv")
    if df is None:
        return False
    fig, axs = plt.subplots(1, 3, figsize=(10, 3.2))
    for i, (var, rotulo) in enumerate([("lat_p95_ms", "p95 do POST /orders"), ("saga_p95_ms", "p95 da conclusão da saga")]):
        d = df[df["variavel"] == var].sort_values("taxa_ofertada_rps")
        axs[0].errorbar(d["taxa_ofertada_rps"], d["valor_mediana"],
                        yerr=[d["valor_mediana"] - d["valor_q1"], d["valor_q3"] - d["valor_mediana"]],
                        color=CORES[i], linestyle=ESTILOS[i], marker="o", capsize=3, label=rotulo)
    axs[0].set_xlabel("Taxa ofertada (req/s)")
    axs[0].set_ylabel("Latência (ms)")
    axs[0].legend()
    for j, (var, ylabel) in enumerate([("cpu_media", "CPU média (núcleos)"), ("mem_max_mb", "Memória de pico (MB)")], start=1):
        d = df[df["variavel"] == var]
        for k, (svc, g) in enumerate(d.groupby("servico", sort=True)):
            g = g.sort_values("taxa_ofertada_rps")
            axs[j].plot(g["taxa_ofertada_rps"], g["valor_mediana"], color=CORES[k % len(CORES)],
                        linestyle=ESTILOS[k % len(ESTILOS)], marker="o", label=svc)
        axs[j].set_xlabel("Taxa ofertada (req/s)")
        axs[j].set_ylabel(ylabel)
    axs[2].legend(loc="upper left", bbox_to_anchor=(1.0, 1.0))
    for ax, letra in zip(axs, "ABC"):
        eixo(ax, letra)
    salvar(fig, saida / "figura2")
    return True


def figura3(base, saida):
    df = ler(base / "figuras" / "figura3.csv")
    if df is None:
        return False
    fig, axs = plt.subplots(3, 1, figsize=(7, 7.5), sharex=True)
    for k, (cond, g) in enumerate(df.groupby("condicao", sort=True)):
        g = g.sort_values("t_rel_s")
        estilo = dict(color=CORES[k % len(CORES)], linestyle=ESTILOS[k % len(ESTILOS)], label=cond)
        axs[0].plot(g["t_rel_s"], g["lat_p95_ms_mediana"], **estilo)
        axs[1].plot(g["t_rel_s"], g["erro_pct_mediana"], **estilo)
        axs[2].step(g["t_rel_s"], g["cb_estado_max_mediana"], where="post", **estilo)
    axs[0].set_ylabel("p95 do POST /orders (ms)")
    axs[1].set_ylabel("Taxa de erro (%)")
    axs[1].set_ylim(bottom=0)
    axs[2].set_ylabel("Estado máximo do\ncircuit breaker na janela")
    axs[2].set_yticks([0, 1, 2], ["fechado", "semiaberto", "aberto"])
    axs[2].set_xlabel("Tempo desde o início da falha (s)")
    axs[0].legend(ncol=5, loc="upper left")
    for ax, letra in zip(axs, "ABC"):
        for x in (0, 60):
            ax.axvline(x, color=PRETO, linewidth=0.8, linestyle=":")
        eixo(ax, letra)
    salvar(fig, saida / "figura3")
    return True


def ecdf(valores):
    x = np.sort(valores)
    return x, np.arange(1, len(x) + 1) / len(x)


def figura4(base, saida):
    df = ler(base / "figuras" / "figura4.csv")
    if df is None:
        return False
    condicoes = [c for c in ["R-ON", "R-OFF"] if c in set(df["condicao"])]
    fig, axs = plt.subplots(1, len(condicoes), figsize=(4.2 * len(condicoes), 3.4), sharey=True, squeeze=False)
    for ax, cond, letra in zip(axs[0], condicoes, "AB"):
        g = df[(df["condicao"] == cond) & df["saga_ms"].notna()]
        for k, coorte in enumerate(["aquecimento", "antes", "falha", "depois"]):
            v = g[g["coorte"] == coorte]["saga_ms"].to_numpy() / 1000.0
            if len(v) == 0:
                continue
            x, y = ecdf(v)
            ax.step(x, y, where="post", color=CORES[k], linestyle=ESTILOS[k], label=coorte)
        ax.set_xscale("log")
        ax.set_xlabel(f"Tempo de conclusão da saga (s), {cond}")
        eixo(ax, letra)
    axs[0][0].set_ylabel("Proporção acumulada")
    axs[0][0].legend(loc="lower right")
    salvar(fig, saida / "figura4")
    return True


def figura5(base, saida):
    df = ler(base / "figuras" / "figura5.csv")
    if df is None:
        return False
    df = df.sort_values("t_rel_s")
    fig, axs = plt.subplots(2, 1, figsize=(7, 5), sharex=True)
    axs[0].plot(df["t_rel_s"], df["fila_media_mediana"], color=PRETO)
    axs[0].set_ylabel("Profundidade de fila\n(mensagens)")
    axs[1].plot(df["t_rel_s"], df["sagas_concluidas_mediana"], color=PRETO)
    axs[1].set_ylabel("Sagas concluídas\n(por janela de 5 s)")
    axs[1].set_xlabel("Tempo desde a parada do serviço (s)")
    marcas = df[df["marcadores"].fillna("").str.contains("parada|retorno")]
    for ax, letra in zip(axs, "AB"):
        for _, m in marcas.iterrows():
            estilo = "--" if "parada" in m["marcadores"] else ":"
            ax.axvline(m["t_rel_s"], color=PRETO, linewidth=0.8, linestyle=estilo)
        eixo(ax, letra)
    salvar(fig, saida / "figura5")
    return True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", default=str(Path(__file__).resolve().parent))
    args = parser.parse_args()
    base = Path(args.base)
    saida = base / "figuras"
    saida.mkdir(parents=True, exist_ok=True)
    for nome, funcao in [("figura2", figura2), ("figura3", figura3), ("figura4", figura4), ("figura5", figura5)]:
        print(f"{nome}: {'gerada' if funcao(base, saida) else 'sem dados'}")


if __name__ == "__main__":
    main()
