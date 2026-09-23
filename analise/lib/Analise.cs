using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.IO.Compression;
using System.Text;
using System.Text.RegularExpressions;

public class K6Req
{
    public long Ts;
    public double Dur;
    public int Status;
    public string Fase;
    public string ErroCodigo;
}

public class K6Dados
{
    public List<K6Req> Posts = new List<K6Req>();
    public List<long> Descartadas = new List<long>();
}

public class ResumoK6
{
    public int N, N201, N503, N504, NConexao, NOutros;
    public double P50 = double.NaN, P95 = double.NaN, P99 = double.NaN, Max = double.NaN;
}

public class MediaMax
{
    public int N;
    public double Media = double.NaN, Max = double.NaN;
}

public static class Analise
{
    static readonly CultureInfo Inv = CultureInfo.InvariantCulture;
    static readonly long Epoca = new DateTimeOffset(1970, 1, 1, 0, 0, 0, TimeSpan.Zero).UtcTicks;

    public static List<string> Campos(string linha)
    {
        var campos = new List<string>();
        var atual = new StringBuilder();
        bool aspas = false;
        for (int i = 0; i < linha.Length; i++)
        {
            char c = linha[i];
            if (aspas)
            {
                if (c == '"')
                {
                    if (i + 1 < linha.Length && linha[i + 1] == '"') { atual.Append('"'); i++; }
                    else aspas = false;
                }
                else atual.Append(c);
            }
            else if (c == '"') aspas = true;
            else if (c == ',') { campos.Add(atual.ToString()); atual.Length = 0; }
            else atual.Append(c);
        }
        campos.Add(atual.ToString());
        return campos;
    }

    static string Tag(string extra, string chave)
    {
        if (string.IsNullOrEmpty(extra)) return "";
        foreach (var parte in extra.Split('&'))
            if (parte.StartsWith(chave + "=")) return parte.Substring(chave.Length + 1);
        return "";
    }

    public static K6Dados LerK6(string caminho)
    {
        var dados = new K6Dados();
        using (var arquivo = File.OpenRead(caminho))
        using (var gz = new GZipStream(arquivo, CompressionMode.Decompress))
        using (var leitor = new StreamReader(gz, Encoding.UTF8))
        {
            var cab = Campos(leitor.ReadLine());
            int iTs = cab.IndexOf("timestamp"), iVal = cab.IndexOf("metric_value"), iErr = cab.IndexOf("error_code"),
                iNome = cab.IndexOf("name"), iStatus = cab.IndexOf("status"), iExtra = cab.IndexOf("extra_tags");
            string linha;
            while ((linha = leitor.ReadLine()) != null)
            {
                bool duracao = linha.StartsWith("http_req_duration,");
                bool descarte = linha.StartsWith("dropped_iterations,");
                if (!duracao && !descarte) continue;
                var f = Campos(linha);
                long ts = long.Parse(f[iTs], Inv);
                double valor = double.Parse(f[iVal], Inv);
                if (descarte)
                {
                    for (int k = 0; k < (int)Math.Round(valor); k++) dados.Descartadas.Add(ts);
                    continue;
                }
                if (f[iNome] != "POST /orders") continue;
                int status;
                if (!int.TryParse(f[iStatus], NumberStyles.Integer, Inv, out status)) status = 0;
                dados.Posts.Add(new K6Req { Ts = ts, Dur = valor, Status = status, Fase = Tag(f[iExtra], "phase"), ErroCodigo = f[iErr] });
            }
        }
        return dados;
    }

    public static double Percentil(List<double> valores, double p)
    {
        if (valores.Count == 0) return double.NaN;
        var s = valores.ToArray();
        Array.Sort(s);
        double h = (s.Length - 1) * p;
        int lo = (int)Math.Floor(h), hi = (int)Math.Ceiling(h);
        return s[lo] + (h - lo) * (s[hi] - s[lo]);
    }

    static ResumoK6 Resumir(IEnumerable<K6Req> reqs)
    {
        var r = new ResumoK6();
        var dur = new List<double>();
        foreach (var q in reqs)
        {
            r.N++;
            dur.Add(q.Dur);
            if (q.Status == 201) r.N201++;
            else if (q.Status == 503) r.N503++;
            else if (q.Status == 504) r.N504++;
            else if (q.Status == 0) r.NConexao++;
            else r.NOutros++;
        }
        if (dur.Count > 0)
        {
            r.P50 = Percentil(dur, 0.50);
            r.P95 = Percentil(dur, 0.95);
            r.P99 = Percentil(dur, 0.99);
            dur.Sort();
            r.Max = dur[dur.Count - 1];
        }
        return r;
    }

    public static ResumoK6 ResumirFase(K6Dados d, string fase)
    {
        var l = new List<K6Req>();
        foreach (var q in d.Posts) if (q.Fase == fase) l.Add(q);
        return Resumir(l);
    }

    public static ResumoK6 ResumirIntervalo(K6Dados d, double ini, double fim)
    {
        var l = new List<K6Req>();
        foreach (var q in d.Posts) if (q.Ts >= ini && q.Ts < fim) l.Add(q);
        return Resumir(l);
    }

    public static int DescartadasEntre(K6Dados d, double ini, double fim)
    {
        int n = 0;
        foreach (var ts in d.Descartadas) if (ts >= ini && ts < fim) n++;
        return n;
    }

    public static long MinTs(K6Dados d)
    {
        long m = long.MaxValue;
        foreach (var q in d.Posts) if (q.Ts < m) m = q.Ts;
        return m;
    }

    public static double ParseTs(string s)
    {
        if (string.IsNullOrWhiteSpace(s)) return double.NaN;
        string t = s.Trim().Replace(' ', 'T');
        t = Regex.Replace(t, @"(\.\d{7})\d+", "$1");
        if (Regex.IsMatch(t, @"[+-]\d\d$")) t += ":00";
        var d = DateTimeOffset.Parse(t, Inv, DateTimeStyles.AssumeUniversal);
        return (d.UtcTicks - Epoca) / 1e7;
    }

    public static double Num(string s)
    {
        if (s == "+Inf") return double.PositiveInfinity;
        if (s == "-Inf") return double.NegativeInfinity;
        if (s == "NaN") return double.NaN;
        return double.Parse(s, Inv);
    }

    public static MediaMax NoIntervalo(double[] t, double[] v, double ini, double fim)
    {
        var r = new MediaMax();
        double soma = 0;
        for (int i = 0; i < t.Length; i++)
        {
            if (t[i] < ini || t[i] >= fim || double.IsNaN(v[i])) continue;
            r.N++;
            soma += v[i];
            if (double.IsNaN(r.Max) || v[i] > r.Max) r.Max = v[i];
        }
        if (r.N > 0) r.Media = soma / r.N;
        return r;
    }

    public static double Aumento(double[] t, double[] v, double ini, double fim)
    {
        double anterior = 0;
        for (int i = 0; i < t.Length && t[i] <= ini; i++)
            if (!double.IsNaN(v[i])) anterior = v[i];
        double total = 0;
        for (int i = 0; i < t.Length; i++)
        {
            if (t[i] <= ini || t[i] > fim || double.IsNaN(v[i])) continue;
            total += v[i] >= anterior ? v[i] - anterior : v[i];
            anterior = v[i];
        }
        return total;
    }

    public static double MediaPrimeirasUltimas(double[] t, double[] v, double ini, double fim, int k, bool ultimas)
    {
        var dentro = new List<double>();
        for (int i = 0; i < t.Length; i++)
            if (t[i] >= ini && t[i] < fim && !double.IsNaN(v[i])) dentro.Add(v[i]);
        if (dentro.Count < k) return double.NaN;
        double soma = 0;
        int inicio = ultimas ? dentro.Count - k : 0;
        for (int i = inicio; i < inicio + k; i++) soma += dentro[i];
        return soma / k;
    }
}
