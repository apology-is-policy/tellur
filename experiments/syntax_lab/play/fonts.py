import sys, json, base64, os, io
from fontTools.ttLib import TTFont
home = os.path.expanduser("~/Library/Fonts/")
out = {}
cmap = None
for key, name in (("regular", "DejaVu Sans Mono for Powerline.ttf"), ("bold", "DejaVu Sans Mono Bold for Powerline.ttf")):
    f = TTFont(home + name)
    if key == "regular":
        cmap = f.getBestCmap(); adv = f["hmtx"]; cell = adv[cmap[ord("x")]][0]
        ok = lambda ch: ord(ch) in cmap and adv[cmap[ord(ch)]][0] == cell
    for t in ("FFTM",):
        if t in f: del f[t]
    f.flavor = "woff2"; buf = io.BytesIO(); f.save(buf)
    out[key] = base64.b64encode(buf.getvalue()).decode()
groups = [
  ("Squares", "■□▢▣▤▥▦▧▨▩▬▭▮▯◧◨◩◪◫◰◱◲◳⊞⊟⊠⊡∎❏❐❑❒"),
  ("Circles", "●○◉◎◌◍◐◑◒◓◔◕◖◗◯⊕⊖⊗⊘⊙⊚⊛⊜⊝"),
  ("Diamonds and stars", "◆◇◈◊⋄❖★☆✦✧⋆✶✴"),
  ("Triangles and pointers", "▲△▴▵▷▸▹►▻▼▽▾▿◁◂◃◄◅◢◣◤◥⊲⊳⊴⊵"),
  ("Arrows", "←↑→↓⇐⇑⇒⇓⇔↦↤↠↞↣↢⇢⇠⇡⇣⇥⇤↴↳↲↰↱↵↶↷↺↻⇄⇅⇆⇉⇇⇈⇊⇀⇁↼↽⇝⇜↯↧↥⟵⟶⟷➔➜➤"),
  ("Logic and math", "∀∃∄∈∉∋∅∧∨¬⊢⊣⊤⊥∴∵≠≤≥≡≢≔≜≝≈∼∝∞÷×±∓∘∙√∑∏∩∪⊂⊃⊆⊇⊎⊓⊔⊏⊑⊸⋯…∶∷∣"),
  ("Rails and boxes", "─│┄┆┈┊╌╎━┃┅┇┉┋╍╏═║┌┐└┘├┤┬┴┼╭╮╯╰╒╓╔╕╖╗╘╙╚╛╜╝╞╟╠╡╢╣╴╵╶╷┏┓┗┛┣┫┝┠┞┟┡┢╱╲╳▏▕"),
  ("Brackets and marks", "⟦⟧⟨⟩⟪⟫«»‹›⌈⌉⌊⌋⌜⌝⌞⌟§¶†‡•‣′″‖¦·⍝⌂⌥⌘⏎✓✗λΔΣΠΦΨΩƒ№"),
]
out["palette"] = [[name, "".join(ch for ch in chars if ok(ch))] for name, chars in groups]
out["dropped"] = "".join(ch for _, chars in groups for ch in chars if not ok(ch))
json.dump(out, open("fonts.json", "w"), ensure_ascii=False)
print("regular", len(out["regular"]) // 1024, "KB  bold", len(out["bold"]) // 1024, "KB  dropped:", out["dropped"])
