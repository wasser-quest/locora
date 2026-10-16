"""Builds assets/mascot/mnemo.sply step files (run with splinely-cli batch)."""
import math, re, sys
OUT="#24140E"; SKIN="#9A5B32"; SKIN_SH="#7A4322"; SKIN_HI="#B97A4C"
LIME="#DDF76A"; TRIM="#C9E85A"
E=1.1  # eye scale
def ellipse(rx, ry):
    k=0.5523
    return (f"M{-rx:g} 0 C {-rx:g} {-ry*k:.2f} {-rx*k:.2f} {-ry:g} 0 {-ry:g} C {rx*k:.2f} {-ry:g} {rx:g} {-ry*k:.2f} {rx:g} 0 "
            f"C {rx:g} {ry*k:.2f} {rx*k:.2f} {ry:g} 0 {ry:g} C {-rx*k:.2f} {ry:g} {-rx:g} {ry*k:.2f} {-rx:g} 0 Z")
def tf(d, sx=1, sy=1):
    res=[]; ix=True
    for tok in re.findall(r"[A-Za-z]|-?\d*\.?\d+", d):
        if tok.isalpha(): res.append(tok); ix=True; continue
        v=float(tok)*(sx if ix else sy); res.append(f"{v:.2f}".rstrip('0').rstrip('.')); ix=not ix
    return " ".join(res)
mx=lambda d: tf(d,-1,1)
stage=sys.argv[2]; steps=[]; add=steps.append
if stage=="1":
    add('new --width 512 --height 512 --name "Mnemo"')
    ids=["afro_core"]; add('node add --kind circle --id afro_core --x 256 --y 205 --radius 150')
    for i in range(16):
        a=2*math.pi*i/16; r=52 if i%2==0 else 46
        x=256+153*math.cos(a); y=min(205+137*math.sin(a),330)
        ids.append(f"afro_b{i}"); add(f'node add --kind circle --id afro_b{i} --x {x:.1f} --y {y:.1f} --radius {r}')
    add('arrange boolean --op union --nodes '+",".join(ids))
if stage=="2":
    add(f'node update path-1 --gradient "radial #70432A@0 #47291A@0.55 #2A170E@1" --stroke {OUT} --stroke-width 6')
    add(f'node add --kind path --id neck --d "M-25 0 L-23 50 L23 50 L25 0 Z" --tx 256 --ty 368 --fill {SKIN_SH} --stroke {OUT} --stroke-width 4')
    add(f'node add --kind path --id body --d "M-150 120 C-146 70 -100 40 -30 32 L30 32 C100 40 146 70 150 120 Z" --tx 256 --ty 392 --gradient "linear 90 #3A3D92 #1E1F55" --stroke {OUT} --stroke-width 5')
    add(f'node add --kind path --id vneck --d "M-24 0 L0 40 L24 0 Z" --tx 256 --ty 418 --fill {SKIN_SH} --stroke {OUT} --stroke-width 3')
    flap="M-3 0 C -22 3 -42 9 -60 20 C -50 26 -42 34 -36 46 C -26 40 -16 38 -5 40 Z"
    add(f'node add --kind path --id collar_l --d "{flap}" --tx 252 --ty 412 --fill #2B2D74 --stroke {TRIM} --stroke-width 4')
    add(f'node add --kind path --id collar_r --d "{mx(flap)}" --tx 260 --ty 412 --fill #2B2D74 --stroke {TRIM} --stroke-width 4')
    add('node add --kind circle --id gem_glow --x 256 --y 462 --radius 22 --gradient "radial rgba(230,255,120,0.85)@0 rgba(230,255,120,0)@1" --pivot center')
    add(f'node add --kind path --id gem --d "M0 -11 L9 0 L0 11 L-9 0 Z" --tx 256 --ty 462 --gradient "radial #FFFFF0@0 {LIME}@0.6 #A8CF2E@1" --stroke {OUT} --stroke-width 2.5 --pivot center')
    face="M-93 -10 C-93 -57 -54 -80 0 -80 C54 -80 93 -57 93 -10 C93 38 79 72 49 99 C32 114 15 123 0 125 C-15 123 -32 114 -49 99 C-79 72 -93 38 -93 -10 Z"
    add(f'node add --kind path --id face --d "{face}" --tx 256 --ty 266 --gradient "linear 90 {SKIN_HI} {SKIN} {SKIN}" --stroke {OUT} --stroke-width 5')
    add(f'node add --kind path --id face_shade --d "M-91 -26 C-62 -6 -30 2 0 4 C30 2 62 -6 91 -26 L91 -60 L-91 -60 Z" --tx 256 --ty 266 --fill {SKIN_SH} --opacity 0.75')
    add(f'node add --kind path --id cheek_shade --d "M93 -10 C93 38 79 72 49 99 C66 70 75 40 78 -6 Z" --tx 256 --ty 266 --fill {SKIN_SH} --opacity 0.55')
    add(f'node add --kind path --id chin_hl --d "M-20 112 Q 0 120 20 112 Q 0 116 -20 112 Z" --tx 256 --ty 266 --fill {SKIN_HI} --opacity 0.6')
    for side,x in (("l",190),("r",322)):
        add(f'node add --kind path --id blush_{side} --d "{ellipse(22,9)}" --tx {x} --ty 336 --fill #F2728A --opacity 0.42')
        for k,dx in enumerate((-9,0,9)):
            add(f'node add --kind path --id hatch_{side}{k} --d "M1 -4 L-2 4" --tx {x+dx} --ty 336 --fill none --stroke #C9455F --stroke-width 2.2 --opacity 0.75')
    brow="M-15 2 Q -1 -6 15 -2"
    add(f'node add --kind path --id brow_l --d "{brow}" --tx 207 --ty 238 --fill none --stroke {OUT} --stroke-width 3.5')
    add(f'node add --kind path --id brow_r --d "{mx(brow)}" --tx 305 --ty 238 --fill none --stroke {OUT} --stroke-width 3.5')
    sclera="M-27 -14 C-20 -30 20 -30 27 -14 C29 4 22 22 12 27 C4 30 -4 30 -12 27 C-22 22 -29 4 -27 -14 Z"
    lash="M-33 -10 C-28 -30 -10 -36 4 -35 C20 -34 30 -26 35 -16 L41 -19 C39 -12 37 -9 33 -6 C26 -22 12 -27 0 -27 C-14 -27 -26 -20 -31 -4 Z"
    for side,x,f in (("l",207,mx),("r",305,lambda d:d)):
        y=292; s=lambda d: tf(f(d),E,E)
        add(f'node add --kind path --id eye_{side} --d "{s(sclera)}" --tx {x} --ty {y} --fill #FFF8F2')
        add(f'node add --kind path --id iris_{side} --d "{tf(ellipse(20,26),E,E)}" --tx {x} --ty {y+3*E:.1f} --gradient "linear 90 #2E0F52 #6E35A8 #C88AF0" --stroke #24103A --stroke-width 2.5')
        add(f'node add --kind path --id iris_ring_{side} --d "{tf(ellipse(14,19),E,E)}" --tx {x} --ty {y+5*E:.1f} --fill none --stroke rgba(255,220,255,0.35) --stroke-width 2')
        add(f'node add --kind path --id pupil_{side} --d "{tf(ellipse(9,12),E,E)}" --tx {x} --ty {y} --fill #1C0A30')
        add(f'node add --kind path --id iris_glow_{side} --d "{tf("M-15 12 Q 0 24 15 12 Q 0 30 -15 12 Z",E,E)}" --tx {x} --ty {y+3*E:.1f} --fill #F3D2FF --opacity 0.85')
        add(f'node add --kind path --id hl1_{side} --d "{tf(ellipse(7.5,9.5),E,E)}" --tx {x-8*E:.1f} --ty {y-9*E:.1f} --fill white')
        add(f'node add --kind circle --id hl2_{side} --x {x+9*E:.1f} --y {y+14*E:.1f} --radius {3.5*E:.1f} --fill white')
        add(f'node add --kind star --id hl3_{side} --x {x+10*E:.1f} --y {y-12*E:.1f} --radius {5*E:.1f} --tips 4 --inner 0.3 --fill white --pivot center')
        add(f'node add --kind path --id lash_{side} --d "{s(lash)}" --tx {x} --ty {y} --fill #1A0E0A')
        add(f'node add --kind path --id flick_{side} --d "{s("M29 -8 L39 -1 L31 -3 Z")}" --tx {x} --ty {y} --fill #1A0E0A')
        add(f'node add --kind path --id lower_{side} --d "{s("M-16 27 Q 0 33 14 28")}" --tx {x} --ty {y} --fill none --stroke #4A2618 --stroke-width 2.5')
        add(f'node add --kind path --id closed_{side} --d "{s("M-28 4 Q 0 18 30 2")}" --tx {x} --ty {y} --fill none --stroke #1A0E0A --stroke-width 5 --opacity 0')
    add('node add --kind path --id nose --d "M1 0 Q -2 5 -3 8" --tx 257 --ty 322 --fill none --stroke #5A2C14 --stroke-width 3')
    add('node add --kind path --id mouth --d "M-12 -1 Q -6 6 0 0 Q 6 6 12 -1" --tx 256 --ty 350 --fill none --stroke #3A1A0E --stroke-width 3.5')
    pts=[(168,234,24),(188,214,28),(212,201,30),(238,194,31),(266,193,31),(293,198,30),(318,209,28),(341,229,24),
         (180,176,38),(220,166,40),(260,162,40),(300,166,40),(336,178,36)]
    for i,(x,y,r) in enumerate(pts): add(f'node add --kind circle --id fr{i} --x {x} --y {y} --radius {r}')
    add('arrange boolean --op union --nodes '+",".join(f"fr{i}" for i in range(len(pts))))
if stage=="3":
    add(f'node update path-2 --gradient "linear 90 #5C3521 #432617" --stroke #2A170E --stroke-width 4.5')
    # curl texture: small hooks on afro and fringe
    import random; rnd=random.Random(7)
    curls=[(150,130),(200,92),(256,78),(312,92),(362,130),(120,190),(392,190),(118,255),(394,255),(140,300),(372,300),
           (175,60),(337,60),(98,160),(414,160),(205,140),(305,140),(232,182),(282,182)]
    for i,(x,y) in enumerate(curls):
        rot=rnd.randint(-40,40); sc=rnd.choice((0.75,0.9,1.0,1.15))
        add(f'node add --kind path --id curl{i} --d "M-9 3 Q -6 -8 3 -6 Q 9 -4 6 4" --tx {x} --ty {y} --rotation {rot} --scale-x {sc} --scale-y {sc} --fill none --stroke #8A5536 --stroke-width 3 --opacity 0.8')
    # soft gloss on the afro: a crescent that follows the top curve (cel style)
    add('node add --kind path --id gloss --d "M-95 18 Q 0 -44 95 18 Q 0 -20 -95 18 Z" --tx 236 --ty 76 --rotation -12 --fill rgba(175,115,80,0.32)')
    # loci hairpin: three glowing dots on the left of the afro
    for i,(x,y) in enumerate([(132,214),(148,188),(170,166)]):
        add(f'node add --kind circle --id pin_glow{i} --x {x} --y {y} --radius 16 --gradient "radial rgba(230,255,120,0.8)@0 rgba(230,255,120,0)@1" --pivot center')
        add(f'node add --kind circle --id pin{i} --x {x} --y {y} --radius 7.5 --gradient "radial #FFFFF0@0 {LIME}@0.6 #A8CF2E@1" --stroke {OUT} --stroke-width 2.5 --pivot center')
    # firefly companion on the right
    fx,fy=402,96
    add(f'node add --kind circle --id ff_glow --x {fx} --y {fy+14} --radius 40 --gradient "radial rgba(236,255,140,0.85)@0 rgba(214,250,110,0.3)@0.45 rgba(200,240,90,0)@1" --pivot center')
    add(f'node add --kind path --id ff_wing_l --d "M0 0 C -6 -2 -24 -10 -26 -22 C -27 -32 -14 -30 -7 -22 C -3 -16 -1 -8 0 0 Z" --tx {fx-4} --ty {fy-2} --fill rgba(225,242,255,0.85) --stroke rgba(110,160,230,0.9) --stroke-width 2')
    add(f'node add --kind path --id ff_wing_r --d "M0 0 C 6 -2 24 -10 26 -22 C 27 -32 14 -30 7 -22 C 3 -16 1 -8 0 0 Z" --tx {fx+4} --ty {fy-2} --fill rgba(225,242,255,0.85) --stroke rgba(110,160,230,0.9) --stroke-width 2')
    add(f'node add --kind path --id ff_tail --d "{ellipse(12,13)}" --tx {fx} --ty {fy+16} --gradient "radial #FFFFF0@0 #F3FF9A@0.45 #BEEB3C@1" --stroke {OUT} --stroke-width 2.5 --pivot center')
    add(f'node add --kind circle --id ff_head --x {fx} --y {fy} --radius 13 --gradient "linear 90 #5A63D8 #2E2F78" --stroke {OUT} --stroke-width 2.5')
    add(f'node add --kind circle --id ff_eye_l --x {fx-4.5} --y {fy-1} --radius 3 --fill white')
    add(f'node add --kind circle --id ff_eye_r --x {fx+4.5} --y {fy-1} --radius 3 --fill white')
    add(f'node add --kind circle --id ff_pupil_l --x {fx-4} --y {fy-0.5} --radius 1.6 --fill #1B1B3A')
    add(f'node add --kind circle --id ff_pupil_r --x {fx+5} --y {fy-0.5} --radius 1.6 --fill #1B1B3A')
if stage=="4":
    eye_parts=lambda sd: [f"eye_{sd}",f"iris_{sd}",f"iris_ring_{sd}",f"pupil_{sd}",f"iris_glow_{sd}",f"hl1_{sd}",f"hl2_{sd}",f"hl3_{sd}",f"lash_{sd}",f"flick_{sd}",f"lower_{sd}"]
    shut=" ".join(f"--state {n}.opacity=0" for sd in "lr" for n in eye_parts(sd))+" --state closed_l.opacity=1 --state closed_r.opacity=1"
    add(f"on --machine blink --from open --to shut --after 3.4 {shut} --blend 0.06")
    add("on --machine blink --from shut --to open --after 0.13 --blend 0.06")
    # eyes follow the pointer a little
    for sd in "lr":
        for n in (f"iris_{sd}",f"iris_ring_{sd}",f"pupil_{sd}",f"iris_glow_{sd}",f"hl1_{sd}",f"hl2_{sd}",f"hl3_{sd}"):
            add(f"follow --target {n} --max 3.5 --strength 0.12 --smoothing 0.12")
    # breathing: the whole figure rises and settles (group animation)
    add("keyframe --clip breathe --target figure --property y --time 1.8 --by -4 --easing ease-in-out --autoplay --loop")
    add("keyframe --clip breathe --target figure --property y --time 3.6 --value 0 --easing ease-in-out")
    # firefly companion hovers, flaps and glows
    add("keyframe --clip hover --target firefly --property y --time 1.3 --by -10 --easing ease-in-out --autoplay --loop")
    add("keyframe --clip hover --target firefly --property y --time 2.6 --value 0 --easing ease-in-out")
    add("keyframe --clip hover --target firefly --property x --time 1.3 --by -4 --easing ease-in-out")
    add("keyframe --clip hover --target firefly --property x --time 2.6 --value 0 --easing ease-in-out")
    add("keyframe --clip flap --target ff_wing_l --property rotation --time 0.1 --value -18 --easing ease-in-out --autoplay --loop")
    add("keyframe --clip flap --target ff_wing_l --property rotation --time 0.2 --value 0 --easing ease-in-out")
    add("keyframe --clip flap --target ff_wing_r --property rotation --time 0.1 --value 18 --easing ease-in-out")
    add("keyframe --clip flap --target ff_wing_r --property rotation --time 0.2 --value 0 --easing ease-in-out")
    for n in ("ff_glow","gem_glow"):
        add(f"keyframe --clip glow --target {n} --property opacity --time 1.2 --value 0.45 --easing ease-in-out --autoplay --loop")
        add(f"keyframe --clip glow --target {n} --property opacity --time 2.4 --value 1 --easing ease-in-out")
    # loci hairpin lights up in order: 1 -> 2 -> 3
    for i in range(3):
        t_on=round(0.35+i*0.5,2)
        for n,lo in ((f"pin_glow{i}",0.15),(f"pin{i}",0.55)):
            extra=" --autoplay --loop" if (i==0 and n.startswith("pin_glow")) else ""
            add(f"keyframe --clip loci --target {n} --property opacity --time 0.01 --value {lo}{extra}")
            add(f"keyframe --clip loci --target {n} --property opacity --time {t_on} --value 1 --easing ease-out")
            add(f"keyframe --clip loci --target {n} --property opacity --time 2.4 --value 1")
            add(f"keyframe --clip loci --target {n} --property opacity --time 3 --value {lo} --easing ease-in")
    # click: the firefly does a happy loop around its spot
    add("clip add --id hop --duration 0.7")
    add("keyframe --clip hop --target firefly --property rotation --time 0.7 --value 360 --easing ease-in-out")
    add("on --event click --target face --play hop")
    add('a11y --title "Mnemo" --description "Mnemo, das Maskottchen von Locora: eine Figur mit braunem Hautton, großem Afro, leuchtender Loci-Haarspange und einem Glühwürmchen als Begleiter."')
    add('a11y --node face --label "Mnemo: klicken, dann dreht das Glühwürmchen eine Runde"')
    add("check")
open(sys.argv[1],'w').write("\n".join(steps)+"\n")
