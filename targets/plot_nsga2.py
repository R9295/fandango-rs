#!/usr/bin/env python3
"""Render the NSGA-II objective-space trace as a self-contained interactive page.

Reads the objective log written by the `yul_kpath` example and emits a standalone HTML
page: each individual plotted as size (nodes) vs scope violations, with a generation
slider, so population convergence is visible directly.

Usage:
    python3 plot_nsga2.py [CSV] [OUT.html] [TARGET_NODES]
    python3 plot_nsga2.py --help

Arguments (all optional, positional):
    CSV           objective log to read          [default: nsga2_population.csv]
    OUT.html      page to write                  [default: nsga2.html]
    TARGET_NODES  size target, for the guide line and for Pareto dominance
                  (objective 1 is |nodes - TARGET_NODES|)   [default: 300]

Generate the CSV first:
    cargo run --example yul_kpath --features yul -- 2 200 42

Keep TARGET_NODES in sync with the constant of the same name in
examples/yul_kpath.rs, or the target line and the front will be computed against the
wrong ideal.
"""
import csv, json, collections, sys

if {"-h", "--help"} & set(sys.argv[1:]):
    print(__doc__.strip())
    sys.exit(0)

args = [a for a in sys.argv[1:] if not a.startswith("-")]
if len(args) > 3:
    sys.exit("too many arguments (expected at most 3); run with --help")

CSV = args[0] if len(args) > 0 else "nsga2_population.csv"
OUT = args[1] if len(args) > 1 else "nsga2.html"
try:
    TARGET_NODES = int(args[2]) if len(args) > 2 else 300
except ValueError:
    sys.exit(f"TARGET_NODES must be an integer, got {args[2]!r}; run with --help")

def pareto_front(points, target):
    """Indices of the non-dominated points.

    Both objectives are minimised: distance from the size target, and violation count.
    Note the first objective is |nodes - target|, NOT nodes -- a 250-node and a 350-node
    program score identically, which is exactly why raw node count is not objective space.
    """
    objs = [(abs(n - target), v) for n, v, *_ in points]
    front = []
    for i, a in enumerate(objs):
        dominated = any(
            b[0] <= a[0] and b[1] <= a[1] and (b[0] < a[0] or b[1] < a[1])
            for j, b in enumerate(objs)
            if j != i
        )
        if not dominated:
            front.append(i)
    return set(front)


try:
    rows = list(csv.DictReader(open(CSV)))
except FileNotFoundError:
    sys.exit(
        f"{CSV}: not found. Generate it first:\n"
        f"    cargo run --example yul_kpath --features yul -- 2 200 42\n"
        f"Run with --help for options."
    )
if not rows:
    sys.exit(f"{CSV}: no rows to plot.")
by_gen = collections.defaultdict(list)
for r in rows:
    by_gen[int(r["generation"])].append([int(r["nodes"]), int(r["violations"])])
gens = sorted(by_gen)
# Each point becomes [nodes, violations, on_pareto_front]
data = []
for g in gens:
    pts = by_gen[g]
    fi = pareto_front(pts, TARGET_NODES)
    data.append([[n, v, 1 if i in fi else 0] for i, (n, v) in enumerate(pts)])
front_sizes = [len({(p[0], p[1]) for p in gen if p[2]}) for gen in data]
distinct = [len({tuple(p[:2]) for p in gen}) for gen in data]
max_nodes = max(p[0] for g in data for p in g)
max_viol = max(p[1] for g in data for p in g)
pop = max(len(g) for g in data)

payload = json.dumps(
    {"gens": gens, "data": data, "distinct": distinct, "frontSizes": front_sizes,
     "maxNodes": max_nodes, "maxViol": max_viol, "pop": pop, "target": TARGET_NODES},
    separators=(",", ":"))

html = f"""<title>NSGA-II objective space — Yul generation</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
  :root{{
    --bg:#e9ebf4; --surface:#f6f7fc; --ink:#181a26; --ink-2:#464a5e; --ink-3:#767b8f;
    --line:#d7d9e6; --line-2:#c7cad9; --accent:#5b46d9; --accent-2:#7a68ee;
    --ghost:#b9bcd0; --warn:#b96f13; --grid:#dcdfeb;
    --shadow:0 1px 2px rgba(24,26,38,.05),0 8px 24px rgba(24,26,38,.06);
  }}
  @media (prefers-color-scheme:dark){{
    :root{{
      --bg:#0f1120; --surface:#161a2c; --ink:#e9eaf4; --ink-2:#aab0c8; --ink-3:#767c98;
      --line:#282d45; --line-2:#333a57; --accent:#9d8bff; --accent-2:#b4a6ff;
      --ghost:#39406040; --warn:#f2ad55; --grid:#232842;
      --shadow:0 1px 2px rgba(0,0,0,.3),0 10px 30px rgba(0,0,0,.35);
    }}
  }}
  :root[data-theme="light"]{{
    --bg:#e9ebf4; --surface:#f6f7fc; --ink:#181a26; --ink-2:#464a5e; --ink-3:#767b8f;
    --line:#d7d9e6; --line-2:#c7cad9; --accent:#5b46d9; --accent-2:#7a68ee;
    --ghost:#b9bcd0; --warn:#b96f13; --grid:#dcdfeb;
  }}
  :root[data-theme="dark"]{{
    --bg:#0f1120; --surface:#161a2c; --ink:#e9eaf4; --ink-2:#aab0c8; --ink-3:#767c98;
    --line:#282d45; --line-2:#333a57; --accent:#9d8bff; --accent-2:#b4a6ff;
    --ghost:#39406040; --warn:#f2ad55; --grid:#232842;
  }}
  *{{box-sizing:border-box}}
  body{{margin:0;background:var(--bg);color:var(--ink);
    font-family:system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;line-height:1.55;
    -webkit-font-smoothing:antialiased}}
  .mono{{font-family:"SF Mono","JetBrains Mono",ui-monospace,Menlo,Consolas,monospace}}
  .wrap{{max-width:980px;margin:0 auto;padding:clamp(20px,4vw,48px) clamp(16px,4vw,28px) 72px}}
  header{{display:flex;justify-content:space-between;align-items:flex-start;gap:16px;margin-bottom:26px}}
  .eyebrow{{font-family:"SF Mono",ui-monospace,Menlo,monospace;font-size:11.5px;letter-spacing:.2em;
    text-transform:uppercase;color:var(--accent);font-weight:600;margin:0 0 10px}}
  h1{{font-family:"SF Mono","JetBrains Mono",ui-monospace,Menlo,monospace;font-weight:600;
    font-size:clamp(24px,4.5vw,38px);letter-spacing:-.02em;margin:0 0 12px;line-height:1.05}}
  .lede{{color:var(--ink-2);max-width:62ch;margin:0;font-size:15.5px}}
  .theme-btn{{flex:none;border:1px solid var(--line-2);background:var(--surface);color:var(--ink-2);
    border-radius:999px;padding:7px 13px;font-size:13px;cursor:pointer;font-family:inherit}}
  .theme-btn:hover{{border-color:var(--accent);color:var(--ink)}}
  .card{{background:var(--surface);border:1px solid var(--line);border-radius:14px;
    box-shadow:var(--shadow);padding:clamp(14px,2.2vw,22px);margin-top:18px}}
  .card-title{{font-family:"SF Mono",ui-monospace,Menlo,monospace;font-size:11px;letter-spacing:.14em;
    text-transform:uppercase;color:var(--ink-3);margin:0 0 14px}}
  .controls{{display:flex;gap:14px;align-items:center;flex-wrap:wrap;margin-bottom:16px}}
  .btn{{border:1px solid var(--line-2);background:var(--surface);color:var(--ink);font-family:inherit;
    font-size:14px;font-weight:550;padding:8px 15px;border-radius:9px;cursor:pointer}}
  .btn:hover{{border-color:var(--accent);color:var(--accent)}}
  .btn.primary{{background:var(--accent);border-color:var(--accent);color:#fff}}
  input[type=range]{{flex:1;min-width:180px;accent-color:var(--accent)}}
  .stats{{display:flex;gap:22px;flex-wrap:wrap;margin-bottom:12px}}
  .stat .k{{font-family:"SF Mono",ui-monospace,Menlo,monospace;font-size:10.5px;letter-spacing:.13em;
    text-transform:uppercase;color:var(--ink-3)}}
  .stat .v{{font-family:"SF Mono",ui-monospace,Menlo,monospace;font-size:22px;font-weight:650;
    font-variant-numeric:tabular-nums;color:var(--ink)}}
  .stat .v.warn{{color:var(--warn)}}
  svg{{width:100%;height:auto;display:block;overflow:visible}}
  .axis{{stroke:var(--line-2);stroke-width:1}}
  .grid{{stroke:var(--grid);stroke-width:1}}
  .tick{{font-family:"SF Mono",ui-monospace,Menlo,monospace;font-size:10.5px;fill:var(--ink-3)}}
  .axlabel{{font-family:"SF Mono",ui-monospace,Menlo,monospace;font-size:11px;fill:var(--ink-2);
    letter-spacing:.08em;text-transform:uppercase}}
  .ghost{{fill:var(--ghost);opacity:.5}}
  .pt{{fill:var(--accent);opacity:.55}}
  .frontpt{{fill:var(--warn);stroke:var(--surface);stroke-width:1.5}}
  .frontline{{stroke:var(--warn);stroke-width:2;fill:none;stroke-linejoin:round;opacity:.75}}
  .targetline{{stroke:var(--warn);stroke-width:1.5;stroke-dasharray:4 4}}
  .sparkline{{stroke:var(--accent);stroke-width:2;fill:none;stroke-linejoin:round}}
  .sparkarea{{fill:var(--accent);opacity:.13}}
  .nowdot{{fill:var(--accent);stroke:var(--surface);stroke-width:2}}
  .legend{{display:flex;gap:18px;flex-wrap:wrap;font-size:13px;color:var(--ink-2);margin-top:12px}}
  .legend .k{{display:inline-flex;gap:7px;align-items:center}}
  .sw{{width:11px;height:11px;border-radius:50%}}
  .note{{color:var(--ink-2);font-size:14.5px;max-width:66ch;margin:14px 0 0}}
  .note b{{color:var(--ink)}}
  @media (prefers-reduced-motion:reduce){{*{{animation:none!important;transition:none!important}}}}
</style>

<div class="wrap">
  <header>
    <div>
      <p class="eyebrow">NSGA-II · objective space</p>
      <h1>Where the population went</h1>
      <p class="lede">Each dot is one individual, placed by its two objectives: program
        <b>size</b> (node count) against <b>scope violations</b>. A healthy multi-objective run
        spreads along a Pareto curve. Watch what this one does instead.</p>
    </div>
    <button class="theme-btn" id="themeBtn">◐ Theme</button>
  </header>

  <div class="card">
    <p class="card-title">objective space · generation <span id="genLabel" class="mono"></span></p>
    <div class="controls">
      <button class="btn primary" id="play">▶ Play</button>
      <input type="range" id="slider" min="0" value="0">
      <button class="btn" id="axis">x: size</button>
      <button class="btn" id="reset">Reset</button>
    </div>
    <div class="stats">
      <div class="stat"><div class="k">generation</div><div class="v" id="sGen">0</div></div>
      <div class="stat"><div class="k">individuals</div><div class="v" id="sPop">0</div></div>
      <div class="stat"><div class="k">distinct points</div><div class="v" id="sDist">0</div></div>
      <div class="stat"><div class="k">pareto front</div><div class="v" id="sFront">0</div></div>
      <div class="stat"><div class="k">median size</div><div class="v" id="sMed">0</div></div>
    </div>
    <svg id="scatter" viewBox="0 0 720 380"></svg>
    <div class="legend">
      <span class="k"><span class="sw" style="background:var(--accent)"></span> this generation</span>
      <span class="k"><span class="sw" style="background:var(--warn);box-shadow:0 0 0 2px var(--surface)"></span> pareto front (non-dominated)</span>
      <span class="k"><span class="sw" style="background:var(--ghost)"></span> earlier generations</span>
      <span class="k"><span class="sw" style="background:var(--warn);border-radius:1px;height:3px;width:16px"></span> size target ({TARGET_NODES} nodes)</span>
    </div>
  </div>

  <div class="card">
    <p class="card-title">diversity collapse · distinct objective points per generation</p>
    <svg id="spark" viewBox="0 0 720 150"></svg>
    <p class="note" id="verdict"></p>
  </div>
</div>

<script>
const D = {payload};
const S = document.getElementById('scatter'), SP = document.getElementById('spark');
const W=720,H=380,PAD={{l:56,r:16,t:14,b:44}};
const yMax = Math.max(D.maxViol,1)*1.15;
let cur = 0, timer = null;
// distMode: plot TRUE objective space, x = |nodes - target|. Raw node count is not an
// objective -- 250 and 350 nodes score identically -- so dominance is only readable here.
let distMode = false;
const xOf = n => distMode ? Math.abs(n - D.target) : n;
function xMaxNow(){{
  return distMode ? Math.max(1, Math.max(...D.data.flat().map(p=>Math.abs(p[0]-D.target))))*1.05
                  : Math.max(D.maxNodes, D.target)*1.05;
}}
const X = v => PAD.l + v/xMaxNow()*(W-PAD.l-PAD.r);
const Y = v => H-PAD.b - v/yMax*(H-PAD.t-PAD.b);

function ticks(max,n){{const step=Math.max(1,Math.ceil(max/n)); const o=[];for(let v=0;v<=max;v+=step)o.push(v);return o;}}

function drawScatter(){{
  let s='';
  const xm = xMaxNow();
  for(const t of ticks(yMax,5)) s+=`<line class="grid" x1="${{PAD.l}}" y1="${{Y(t)}}" x2="${{W-PAD.r}}" y2="${{Y(t)}}"/><text class="tick" x="${{PAD.l-9}}" y="${{Y(t)+3.5}}" text-anchor="end">${{t}}</text>`;
  for(const t of ticks(xm,6)) s+=`<text class="tick" x="${{X(t)}}" y="${{H-PAD.b+17}}" text-anchor="middle">${{t}}</text>`;
  s+=`<line class="axis" x1="${{PAD.l}}" y1="${{H-PAD.b}}" x2="${{W-PAD.r}}" y2="${{H-PAD.b}}"/>`;
  s+=`<line class="axis" x1="${{PAD.l}}" y1="${{PAD.t}}" x2="${{PAD.l}}" y2="${{H-PAD.b}}"/>`;
  // In distance mode the ideal is x=0 (exactly on target); in size mode it is the target.
  const tx = distMode ? X(0) : X(D.target);
  s+=`<line class="targetline" x1="${{tx}}" y1="${{PAD.t}}" x2="${{tx}}" y2="${{H-PAD.b}}"/>`;
  for(let g=0;g<cur;g++) for(const p of D.data[g]) s+=`<circle class="ghost" cx="${{X(xOf(p[0]))}}" cy="${{Y(p[1])}}" r="3"/>`;

  const pts=D.data[cur];
  // The Pareto front, drawn as an actual curve: sort the non-dominated points by the
  // first objective and connect them. A healthy run traces a trade-off curve; a converged
  // one shows a single dot.
  const front = pts.filter(p=>p[2]).sort((a,b)=>Math.abs(a[0]-D.target)-Math.abs(b[0]-D.target));
  if(distMode && front.length>1){{
    let d=`M ${{X(xOf(front[0][0]))}} ${{Y(front[0][1])}}`;
    for(let i=1;i<front.length;i++) d+=` L ${{X(xOf(front[i][0]))}} ${{Y(front[i][1])}}`;
    s+=`<path class="frontline" d="${{d}}"/>`;
  }}
  for(const p of pts) if(!p[2]) s+=`<circle class="pt" cx="${{X(xOf(p[0]))}}" cy="${{Y(p[1])}}" r="5"><title>${{p[0]}} nodes (dist ${{Math.abs(p[0]-D.target)}}), ${{p[1]}} violations</title></circle>`;
  for(const p of front) s+=`<circle class="frontpt" cx="${{X(xOf(p[0]))}}" cy="${{Y(p[1])}}" r="6.5"><title>PARETO FRONT — ${{p[0]}} nodes (dist ${{Math.abs(p[0]-D.target)}}), ${{p[1]}} violations</title></circle>`;

  s+=`<text class="axlabel" x="${{(W+PAD.l)/2}}" y="${{H-4}}" text-anchor="middle">${{distMode?'objective 1: |nodes − target|  (0 = ideal)':'program size (nodes)'}}</text>`;
  s+=`<text class="axlabel" transform="rotate(-90 14 ${{H/2}})" x="14" y="${{H/2}}" text-anchor="middle">objective 2: violations</text>`;
  S.innerHTML=s;
  const sizes=pts.map(p=>p[0]).sort((a,b)=>a-b);
  document.getElementById('sGen').textContent=cur;
  document.getElementById('sPop').textContent=pts.length;
  const dist=D.distinct[cur];
  const de=document.getElementById('sDist'); de.textContent=dist; de.className='v'+(dist<=5?' warn':'');
  const fe=document.getElementById('sFront'); fe.textContent=D.frontSizes[cur];
  fe.className='v'+(D.frontSizes[cur]<=1?' warn':'');
  document.getElementById('sMed').textContent=sizes[Math.floor(sizes.length/2)];
  document.getElementById('genLabel').textContent=`${{cur}} / ${{D.gens.length-1}}`;
  drawSpark();
}}

function drawSpark(){{
  const w=720,h=150,p={{l:56,r:16,t:12,b:30}};
  const n=D.distinct.length, mx=Math.max(...D.distinct);
  const sx=i=> p.l + (n===1?0:i/(n-1))*(w-p.l-p.r);
  const sy=v=> h-p.b - v/mx*(h-p.t-p.b);
  let s='';
  for(const t of ticks(mx,4)) s+=`<line class="grid" x1="${{p.l}}" y1="${{sy(t)}}" x2="${{w-p.r}}" y2="${{sy(t)}}"/><text class="tick" x="${{p.l-9}}" y="${{sy(t)+3.5}}" text-anchor="end">${{t}}</text>`;
  let d=`M ${{sx(0)}} ${{sy(D.distinct[0])}}`;
  for(let i=1;i<n;i++) d+=` L ${{sx(i)}} ${{sy(D.distinct[i])}}`;
  s+=`<path class="sparkarea" d="${{d}} L ${{sx(n-1)}} ${{h-p.b}} L ${{sx(0)}} ${{h-p.b}} Z"/><path class="sparkline" d="${{d}}"/>`;
  s+=`<line class="axis" x1="${{p.l}}" y1="${{h-p.b}}" x2="${{w-p.r}}" y2="${{h-p.b}}"/>`;
  for(let i=0;i<n;i++) s+=`<text class="tick" x="${{sx(i)}}" y="${{h-p.b+16}}" text-anchor="middle">${{i}}</text>`;
  s+=`<circle class="nowdot" cx="${{sx(cur)}}" cy="${{sy(D.distinct[cur])}}" r="5"/>`;
  s+=`<text class="axlabel" x="${{(w+p.l)/2}}" y="${{h-2}}" text-anchor="middle">generation</text>`;
  SP.innerHTML=s;
  document.getElementById('verdict').innerHTML =
    `Distinct objective points fall from <b>${{D.distinct[0]}}</b> at generation 0 to `+
    `<b>${{D.distinct[D.distinct.length-1]}}</b> by generation ${{D.gens.length-1}} — from `+
    `${{D.pop}} individuals. The population converges onto a handful of clones, so the Pareto `+
    `front is a point rather than a curve and the diversity hook has nothing left to spread.`;
}}

const slider=document.getElementById('slider');
slider.max=D.gens.length-1;
slider.addEventListener('input',e=>{{cur=+e.target.value;drawScatter();}});
document.getElementById('reset').addEventListener('click',()=>{{stop();cur=0;slider.value=0;drawScatter();}});
document.getElementById('axis').addEventListener('click',e=>{{
  distMode=!distMode; e.target.textContent = distMode ? 'x: |n−target|' : 'x: size'; drawScatter();
}});
function stop(){{if(timer){{clearInterval(timer);timer=null;}}document.getElementById('play').textContent='▶ Play';}}
document.getElementById('play').addEventListener('click',()=>{{
  if(timer){{stop();return;}}
  document.getElementById('play').textContent='❚❚ Pause';
  timer=setInterval(()=>{{cur=(cur+1)%D.gens.length;slider.value=cur;drawScatter();if(cur===D.gens.length-1)stop();}},700);
}});
const tb=document.getElementById('themeBtn');
function sysDark(){{return matchMedia('(prefers-color-scheme:dark)').matches;}}
function curDark(){{const t=document.documentElement.getAttribute('data-theme');return t?t==='dark':sysDark();}}
tb.addEventListener('click',()=>{{document.documentElement.setAttribute('data-theme',curDark()?'light':'dark');drawScatter();}});
drawScatter();
</script>
"""

open(OUT, "w").write(html)
print(f"wrote {OUT}: {len(gens)} generations, {len(rows)} individuals, pop={pop}")
print(f"distinct per gen: {distinct}")
