"""Builds assets/mascot/simo.sply step files (stages 1-4) for splinely-cli batch."""
import math, re, sys, random
OUT="#24140E"
SKIN="#C68A5E"; SKIN_SH="#A66C42"; SKIN_HI="#DDA67C"
LIME="#DDF76A"; TRIM="#C9E85A"; CREAM="#F6F1E6"
E=0.95; FS=0.92  # eye scale, face scale
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
def capsule(w, l):
    r=w/2
    return f"M{-r:g} 0 C {-r:g} {-r*1.1:.1f} {r:g} {-r*1.1:.1f} {r:g} 0 L{r:g} {l:g} C {r:g} {l+r*1.1:.1f} {-r:g} {l+r*1.1:.1f} {-r:g} {l:g} Z"
stage=sys.argv[2]; steps=[]; add=steps.append
HX,HY=256,252  # face centre
if stage=="1":
    add('new --width 512 --height 768 --name "Simo"')
    ids=["afro_core"]; add('node add --kind circle --id afro_core --x 256 --y 196 --radius 132')
    for i in range(16):
        a=2*math.pi*i/16; r=46 if i%2==0 else 40
        x=256+140*math.cos(a); y=min(196+124*math.sin(a),300)
        ids.append(f"afro_b{i}"); add(f'node add --kind circle --id afro_b{i} --x {x:.1f} --y {y:.1f} --radius {r}')
    add('arrange boolean --op union --nodes '+",".join(ids))
if stage=="2":
    add(f'node update path-1 --gradient "radial #70432A@0 #47291A@0.55 #2A170E@1" --stroke {OUT} --stroke-width 6')
    # legs, hips, shoes (behind the jacket)
    for side,x,rot in (("l",230,3),("r",282,-3)):
        add(f'node add --kind path --id leg_{side} --d "{capsule(46,112)}" --tx {x} --ty 566 --rotation {rot} --gradient "linear 90 #2B2D74 #1C1D50" --stroke {OUT} --stroke-width 5')
    add(f'node add --kind path --id hips --d "M-66 0 L66 0 L58 44 L-58 44 Z" --tx 256 --ty 540 --gradient "linear 90 #2B2D74 #23245E" --stroke {OUT} --stroke-width 5')
    shoe="M-34 2 C -34 -16 16 -20 30 -6 C 38 2 34 16 20 16 L-26 16 C -34 16 -36 10 -34 2 Z"
    for side,x,f in (("l",222,mx),("r",290,lambda d:d)):
        add(f'node add --kind path --id shoe_{side} --d "{f(shoe)}" --tx {x} --ty 690 --fill {CREAM} --stroke {OUT} --stroke-width 4.5')
        add(f'node add --kind path --id shoe_stripe_{side} --d "{f("M-22 6 L18 6")}" --tx {x} --ty 690 --fill none --stroke {TRIM} --stroke-width 4')
    # neck, jacket, shirt, collar, zip
    add(f'node add --kind path --id neck --d "M-21 0 L-20 44 L20 44 L21 0 Z" --tx 256 --ty 346 --fill {SKIN_SH} --stroke {OUT} --stroke-width 4')
    add(f'node add --kind path --id jacket --d "M-62 0 C -86 4 -100 22 -102 54 L-96 178 C -60 188 60 188 96 178 L102 54 C 100 22 86 4 62 0 Z" --tx 256 --ty 380 --gradient "linear 90 #3A3D92 #1E1F55" --stroke {OUT} --stroke-width 5')
    add(f'node add --kind path --id shirt --d "M-26 0 L0 46 L26 0 Z" --tx 256 --ty 382 --fill {CREAM} --stroke {OUT} --stroke-width 3')
    flap="M-3 0 C -18 3 -34 8 -48 17 C -40 22 -34 29 -30 38 C -21 33 -13 32 -4 33 Z"
    add(f'node add --kind path --id collar_l --d "{flap}" --tx 252 --ty 380 --fill #2B2D74 --stroke {TRIM} --stroke-width 3.5')
    add(f'node add --kind path --id collar_r --d "{mx(flap)}" --tx 260 --ty 380 --fill #2B2D74 --stroke {TRIM} --stroke-width 3.5')
    add(f'node add --kind path --id zip --d "M0 0 L0 128" --tx 256 --ty 430 --fill none --stroke {TRIM} --stroke-width 3')
    add(f'node add --kind path --id pocket_l --d "M-20 0 L20 0" --tx 212 --ty 500 --fill none --stroke rgba(255,255,255,0.25) --stroke-width 3')
    add(f'node add --kind path --id pocket_r --d "M-20 0 L20 0" --tx 300 --ty 500 --fill none --stroke rgba(255,255,255,0.25) --stroke-width 3')
    # loci pin on the jacket: three lights
    for i,(x,y) in enumerate([(212,420),(224,436),(236,452)]):
        add(f'node add --kind circle --id pin_glow{i} --x {x} --y {y} --radius 12 --gradient "radial rgba(230,255,120,0.8)@0 rgba(230,255,120,0)@1" --pivot center')
        add(f'node add --kind circle --id pin{i} --x {x} --y {y} --radius 5.5 --gradient "radial #FFFFF0@0 {LIME}@0.6 #A8CF2E@1" --stroke {OUT} --stroke-width 2 --pivot center')
    # arms: drawn in each arm bone's local frame (origin = shoulder)
    for side in "lr":
        add(f'node add --kind path --id sleeve_{side} --d "{capsule(38,118)}" --gradient "linear 0 #34378A #23245E" --stroke {OUT} --stroke-width 5')
        add(f'node add --kind path --id cuff_{side} --d "M-20 0 L20 0" --ty 112 --fill none --stroke {TRIM} --stroke-width 6')
    # lantern hangs from the right hand (local origin = hand)
    add('node add --kind circle --id lantern_halo --x 0 --y 44 --radius 56 --gradient "radial rgba(236,255,140,0.75)@0 rgba(214,250,110,0.25)@0.5 rgba(200,240,90,0)@1" --pivot center')
    add(f'node add --kind path --id lantern_string --d "M0 0 L0 20" --fill none --stroke {OUT} --stroke-width 3')
    add(f'node add --kind path --id lantern_cap --d "M-15 0 L15 0 L11 9 L-11 9 Z" --ty 20 --fill #2B2D74 --stroke {OUT} --stroke-width 3')
    add(f'node add --kind rect --id lantern_glass --x -13 --y 29 --width 26 --height 34 --rx 9 --gradient "radial #FFFFF0@0 #F3FF9A@0.5 #BEEB3C@1" --stroke {OUT} --stroke-width 3')
    add(f'node add --kind path --id lantern_bars --d "M-5 31 L-5 61 M5 31 L5 61" --fill none --stroke rgba(46,47,120,0.55) --stroke-width 2')
    add(f'node add --kind path --id lantern_base --d "M-11 0 L11 0 L15 7 L-15 7 Z" --ty 63 --fill #2B2D74 --stroke {OUT} --stroke-width 3')
    for side in "lr":
        add(f'node add --kind circle --id hand_{side} --x 0 --y 134 --radius 15 --fill {SKIN} --stroke {OUT} --stroke-width 4')
    # face
    face="M-93 -10 C-93 -57 -54 -80 0 -80 C54 -80 93 -57 93 -10 C93 34 82 66 54 94 C36 111 18 120 0 122 C-18 120 -36 111 -54 94 C-82 66 -93 34 -93 -10 Z"
    add(f'node add --kind path --id face --d "{tf(face,FS,FS)}" --tx {HX} --ty {HY} --gradient "linear 90 {SKIN_HI} {SKIN} {SKIN}" --stroke {OUT} --stroke-width 5')
    add(f'node add --kind path --id face_shade --d "{tf("M-91 -26 C-62 -6 -30 2 0 4 C30 2 62 -6 91 -26 L91 -60 L-91 -60 Z",FS,FS)}" --tx {HX} --ty {HY} --fill {SKIN_SH} --opacity 0.7')
    add(f'node add --kind path --id cheek_shade --d "{tf("M93 -10 C93 34 82 66 54 94 C70 64 77 36 79 -6 Z",FS,FS)}" --tx {HX} --ty {HY} --fill {SKIN_SH} --opacity 0.5')
    for side,x in (("l",188),("r",324)):
        add(f'node add --kind path --id blush_{side} --d "{ellipse(18,7)}" --tx {x} --ty 314 --fill #E8707E --opacity 0.25')
    brow="M-18 3 Q -2 -4 18 -1 L18 4 Q -2 1 -18 7 Z"
    add(f'node add --kind path --id brow_l --d "{mx(brow)}" --tx 210 --ty 236 --fill {OUT}')
    add(f'node add --kind path --id brow_r --d "{brow}" --tx 302 --ty 236 --fill {OUT}')
    sclera="M-27 -14 C-20 -30 20 -30 27 -14 C29 4 22 22 12 27 C4 30 -4 30 -12 27 C-22 22 -29 4 -27 -14 Z"
    lash="M-31 -8 C-27 -28 -10 -33 4 -32 C20 -31 30 -24 34 -13 L38 -14 C36 -9 34 -7 31 -5 C25 -20 12 -24 0 -24 C-13 -24 -25 -18 -29 -3 Z"
    for side,x,f in (("l",210,mx),("r",302,lambda d:d)):
        y=280; s=lambda d: tf(f(d),E,E)
        add(f'node add --kind path --id eye_{side} --d "{s(sclera)}" --tx {x} --ty {y} --fill #FFF8F2')
        add(f'node add --kind path --id iris_{side} --d "{tf(ellipse(20,26),E,E)}" --tx {x} --ty {y+3*E:.1f} --gradient "linear 90 #4A1E06 #B0651A #F2CF7A" --stroke #2E1404 --stroke-width 2.5')
        add(f'node add --kind path --id iris_ring_{side} --d "{tf(ellipse(14,19),E,E)}" --tx {x} --ty {y+5*E:.1f} --fill none --stroke rgba(255,240,200,0.35) --stroke-width 2')
        add(f'node add --kind path --id pupil_{side} --d "{tf(ellipse(9,12),E,E)}" --tx {x} --ty {y} --fill #2A1004')
        add(f'node add --kind path --id iris_glow_{side} --d "{tf("M-15 12 Q 0 24 15 12 Q 0 30 -15 12 Z",E,E)}" --tx {x} --ty {y+3*E:.1f} --fill #FFE9B8 --opacity 0.85')
        add(f'node add --kind path --id hl1_{side} --d "{tf(ellipse(7.5,9.5),E,E)}" --tx {x-8*E:.1f} --ty {y-9*E:.1f} --fill white')
        add(f'node add --kind circle --id hl2_{side} --x {x+9*E:.1f} --y {y+14*E:.1f} --radius {3.5*E:.1f} --fill white')
        add(f'node add --kind star --id hl3_{side} --x {x+10*E:.1f} --y {y-12*E:.1f} --radius {4.5*E:.1f} --tips 4 --inner 0.3 --fill white --pivot center')
        add(f'node add --kind path --id lash_{side} --d "{s("M-29 -9 C -20 -27 16 -29 30 -13 L35 -15")}" --tx {x} --ty {y} --fill none --stroke #1A0E0A --stroke-width 5')
        add(f'node add --kind path --id lower_{side} --d "{s("M-14 27 Q 0 32 12 28")}" --tx {x} --ty {y} --fill none --stroke #6A3A1E --stroke-width 2.2')
        add(f'node add --kind path --id closed_{side} --d "{s("M-28 4 Q 0 18 30 2")}" --tx {x} --ty {y} --fill none --stroke #1A0E0A --stroke-width 5 --opacity 0')
    add('node add --kind path --id nose --d "M1 0 Q -2 5 -4 8 L0 9" --tx 257 --ty 302 --fill none --stroke #7A4322 --stroke-width 3')
    add('node add --kind path --id mouth --d "M-13 -2 Q 0 10 13 -2" --tx 256 --ty 330 --fill none --stroke #3A1A0E --stroke-width 3.5')
    pts=[(176,226,22),(194,206,26),(216,193,28),(241,187,29),(268,186,29),(294,191,28),(318,202,26),(338,222,22),
         (190,170,34),(228,160,36),(266,156,36),(304,162,34),(334,176,30)]
    for i,(x,y,r) in enumerate(pts): add(f'node add --kind circle --id fr{i} --x {x} --y {y} --radius {r}')
    add('arrange boolean --op union --nodes '+",".join(f"fr{i}" for i in range(len(pts))))
if stage=="3":
    add('node update path-2 --gradient "linear 90 #5C3521 #432617" --stroke #2A170E --stroke-width 4.5')
    rnd=random.Random(11)
    curls=[(160,120),(205,84),(256,72),(307,84),(352,120),(132,180),(380,180),(136,246),(376,246),(176,64),(336,64),
           (112,150),(400,150),(214,132),(298,132),(248,170),(150,280),(362,280)]
    for i,(x,y) in enumerate(curls):
        rot=rnd.randint(-40,40); sc=rnd.choice((0.75,0.9,1.0,1.1))
        add(f'node add --kind path --id curl{i} --d "M-9 3 Q -6 -8 3 -6 Q 9 -4 6 4" --tx {x} --ty {y} --rotation {rot} --scale-x {sc} --scale-y {sc} --fill none --stroke #8A5536 --stroke-width 3 --opacity 0.8')
    add('node add --kind path --id gloss --d "M-85 16 Q 0 -40 85 16 Q 0 -18 -85 16 Z" --tx 238 --ty 74 --rotation -12 --fill rgba(175,115,80,0.32)')
    fx,fy=394,92
    add(f'node add --kind circle --id ff_glow --x {fx} --y {fy+14} --radius 38 --gradient "radial rgba(236,255,140,0.85)@0 rgba(214,250,110,0.3)@0.45 rgba(200,240,90,0)@1" --pivot center')
    add(f'node add --kind path --id ff_wing_l --d "M0 0 C -6 -2 -24 -10 -26 -22 C -27 -32 -14 -30 -7 -22 C -3 -16 -1 -8 0 0 Z" --tx {fx-4} --ty {fy-2} --fill rgba(225,242,255,0.85) --stroke rgba(110,160,230,0.9) --stroke-width 2')
    add(f'node add --kind path --id ff_wing_r --d "M0 0 C 6 -2 24 -10 26 -22 C 27 -32 14 -30 7 -22 C 3 -16 1 -8 0 0 Z" --tx {fx+4} --ty {fy-2} --fill rgba(225,242,255,0.85) --stroke rgba(110,160,230,0.9) --stroke-width 2')
    add(f'node add --kind path --id ff_tail --d "{ellipse(12,13)}" --tx {fx} --ty {fy+16} --gradient "radial #FFFFF0@0 #F3FF9A@0.45 #BEEB3C@1" --stroke {OUT} --stroke-width 2.5 --pivot center')
    add(f'node add --kind circle --id ff_head --x {fx} --y {fy} --radius 13 --gradient "linear 90 #5A63D8 #2E2F78" --stroke {OUT} --stroke-width 2.5')
    add(f'node add --kind circle --id ff_eye_l --x {fx-4.5} --y {fy-1} --radius 3 --fill white')
    add(f'node add --kind circle --id ff_eye_r --x {fx+4.5} --y {fy-1} --radius 3 --fill white')
    add(f'node add --kind circle --id ff_pupil_l --x {fx-4} --y {fy-0.5} --radius 1.6 --fill #1B1B3A')
    add(f'node add --kind circle --id ff_pupil_r --x {fx+5} --y {fy-0.5} --radius 1.6 --fill #1B1B3A')
    # rig: torso (breathing) with two arm bones at the shoulders
    add('bone add --id torso --x 0 --y 0 --length 1')
    add('bone add --id arm_l --parent torso --x 171 --y 398 --rotation 13 --length 1')
    add('bone add --id arm_r --parent torso --x 345 --y 398 --rotation -13 --length 1')
    # the lantern hangs from the hand and cancels the arm's turn so it stays upright
    add('bone add --id lantern --parent arm_r --x -1 --y 134 --rotation 13 --length 1')
if stage=="bind":
    arm={"l":["sleeve_l","cuff_l","hand_l"],"r":["sleeve_r","cuff_r","hand_r"]}
    lantern=["lantern_halo","lantern_string","lantern_cap","lantern_glass","lantern_bars","lantern_base"]
    for n in arm["l"]: add(f"bone bind --node {n} --bone arm_l")
    for n in arm["r"]: add(f"bone bind --node {n} --bone arm_r")
    for n in lantern: add(f"bone bind --node {n} --bone lantern")
    for n in sys.argv[3].split(","):
        if n not in arm["l"]+arm["r"]+lantern and not n.startswith("ff_"):
            add(f"bone bind --node {n} --bone torso")
if stage=="4":
    eye_parts=lambda sd: [f"eye_{sd}",f"iris_{sd}",f"iris_ring_{sd}",f"pupil_{sd}",f"iris_glow_{sd}",f"hl1_{sd}",f"hl2_{sd}",f"hl3_{sd}",f"lash_{sd}",f"lower_{sd}"]
    shut=" ".join(f"--state {n}.opacity=0" for sd in "lr" for n in eye_parts(sd))+" --state closed_l.opacity=1 --state closed_r.opacity=1"
    add(f"on --machine blink --from open --to shut --after 3.8 {shut} --blend 0.06")
    add("on --machine blink --from shut --to open --after 0.13 --blend 0.06")
    for sd in "lr":
        for n in (f"iris_{sd}",f"iris_ring_{sd}",f"pupil_{sd}",f"iris_glow_{sd}",f"hl1_{sd}",f"hl2_{sd}",f"hl3_{sd}"):
            add(f"follow --target {n} --max 3.5 --strength 0.12 --smoothing 0.12")
    add("keyframe --clip breathe --target torso --property y --time 1.8 --by -3 --easing ease-in-out --autoplay --loop")
    add("keyframe --clip breathe --target torso --property y --time 3.6 --value 0 --easing ease-in-out")
    # lantern sways like a pendulum: the arm swings a little, the lantern lags behind
    add("keyframe --clip sway --target arm_r --property rotation --time 1.2 --value -17 --easing ease-in-out --autoplay --loop")
    add("keyframe --clip sway --target arm_r --property rotation --time 2.4 --value -13 --easing ease-in-out")
    add("keyframe --clip sway --target lantern --property rotation --time 0.3 --value 13")
    add("keyframe --clip sway --target lantern --property rotation --time 1.5 --value 21 --easing ease-in-out")
    add("keyframe --clip sway --target lantern --property rotation --time 2.4 --value 13 --easing ease-in-out")
    add("keyframe --clip sway --target arm_l --property rotation --time 1.2 --value 15 --easing ease-in-out")
    add("keyframe --clip sway --target arm_l --property rotation --time 2.4 --value 13 --easing ease-in-out")
    add("keyframe --clip hover --target firefly --property y --time 1.3 --by -10 --easing ease-in-out --autoplay --loop")
    add("keyframe --clip hover --target firefly --property y --time 2.6 --value 0 --easing ease-in-out")
    add("keyframe --clip flap --target ff_wing_l --property rotation --time 0.1 --value -18 --easing ease-in-out --autoplay --loop")
    add("keyframe --clip flap --target ff_wing_l --property rotation --time 0.2 --value 0 --easing ease-in-out")
    add("keyframe --clip flap --target ff_wing_r --property rotation --time 0.1 --value 18 --easing ease-in-out")
    add("keyframe --clip flap --target ff_wing_r --property rotation --time 0.2 --value 0 --easing ease-in-out")
    for n in ("ff_glow","lantern_halo"):
        add(f"keyframe --clip glow --target {n} --property opacity --time 1.2 --value 0.5 --easing ease-in-out --autoplay --loop")
        add(f"keyframe --clip glow --target {n} --property opacity --time 2.4 --value 1 --easing ease-in-out")
    for i in range(3):
        t_on=round(0.35+i*0.5,2)
        for n,lo in ((f"pin_glow{i}",0.15),(f"pin{i}",0.55)):
            extra=" --autoplay --loop" if (i==0 and n.startswith("pin_glow")) else ""
            add(f"keyframe --clip loci --target {n} --property opacity --time 0.01 --value {lo}{extra}")
            add(f"keyframe --clip loci --target {n} --property opacity --time {t_on} --value 1 --easing ease-out")
            add(f"keyframe --clip loci --target {n} --property opacity --time 2.4 --value 1")
            add(f"keyframe --clip loci --target {n} --property opacity --time 3 --value {lo} --easing ease-in")
    add("clip add --id hop --duration 0.7")
    add("keyframe --clip hop --target firefly --property rotation --time 0.7 --value 360 --easing ease-in-out")
    add("on --event click --target face --play hop")
    add('a11y --title "Simo" --description "Simo, Figur von Locora: ein Mann mit hellbraunem Hautton und Afro in voller Größe, mit einer Laterne voller Glühwürmchen-Licht und drei leuchtenden Loci am Kragen."')
    add('a11y --node face --label "Simo: klicken, dann dreht das Glühwürmchen eine Runde"')
    add("check")
open(sys.argv[1],'w').write("\n".join(steps)+"\n")
