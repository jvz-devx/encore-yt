from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
jobs = [("Inter", 14, 400, "Regular"), ("Inter", 14, 500, "Medium"), ("Inter", 14, 600, "SemiBold"),
        ("Inter", 14, 700, "Bold"), ("Inter Display", 32, 700, "Bold")]
for family, opsz, wght, style in jobs:
    f = TTFont("InterVariable.ttf")
    inst = instantiateVariableFont(f, {"wght": wght, "opsz": opsz})
    name = inst["name"]
    for rec in list(name.names):
        if rec.nameID in (1, 2, 3, 4, 6, 16, 17, 25) or rec.nameID >= 256:
            name.removeNames(nameID=rec.nameID)
    full = f"{family} {style}"
    ps = full.replace(" ", "-").replace("Inter-Display", "InterDisplay")
    legacy_family = family if style in ("Regular", "Bold") else f"{family} {style}"
    legacy_style = style if style in ("Regular", "Bold") else "Regular"
    for nid, val in [(1, legacy_family), (2, legacy_style), (3, f"4.001;{ps}"), (4, full), (6, ps), (16, family), (17, style)]:
        name.setName(val, nid, 3, 1, 0x409)
    inst["OS/2"].usWeightClass = wght
    sel = inst["OS/2"].fsSelection & ~0b1100001
    sel |= 0b100000 if style == "Bold" else (0b1000000 if style == "Regular" else 0)
    inst["OS/2"].fsSelection = sel
    inst["head"].macStyle = 1 if style == "Bold" else 0
    if "STAT" in inst: del inst["STAT"]
    out = f"{ps}.ttf"
    inst.save(out)
    print(out, wght, opsz)
