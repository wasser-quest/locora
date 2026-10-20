function attach(svg, host) {
    var timedClips = [], timedCancel = null, timedTicker = null, timed = false;
    function animationDuration() {
      var clips = svg.splinely && svg.splinely.sourceProject.clips;
      return clips && clips.length ? Math.max.apply(null, clips.map(function (clip) { return clip.duration; })) : 0;
    }
    function timedReply(field, value) {
      var reply = { locora: 1 };
      reply[field] = value;
      host.reply(reply);
    }
    function pauseTimed() {
      if (timedCancel) { timedCancel(); timedCancel = null; }
      if (timedTicker) { clearInterval(timedTicker); timedTicker = null; }
      if (svg.splinely) svg.splinely.timeline.pause();
    }
    // The app forwards the pointer as coordinates. These helpers rebuild the DOM sequence
    // a browser would produce: enter/leave along the ancestor chain, a press on one element,
    // moves to that element while pressed, and a click on the nearest common ancestor.
    var hovered = null, pressed = null;
    function chainOf(node) {
      var out = [];
      for (var n = node; n && n !== svg; n = n.parentNode) out.push(n);
      return out;
    }
    function targetAt(x, y) {
      // x and y are the graphic's own coordinates (its box, in CSS pixels). The page hit-tests in viewport
      // coordinates; in a frame the box starts at the origin, in the page it does not.
      var box = svg.getBoundingClientRect();
      // The page keeps the graphic out of pointer hit tests (pointer-events: none on its box), so the
      // graphic is hit-testable for this one lookup only. Touches still arrive through the app.
      var previous = svg.style.pointerEvents;
      svg.style.pointerEvents = 'auto';
      var el = document.elementFromPoint(box.left + x, box.top + y);
      svg.style.pointerEvents = previous;
      return el && el !== svg && svg.contains(el) ? el : null;
    }
    function pointerEvent(type, x, y, bubbles) {
      return new PointerEvent(type, {
        clientX: x, clientY: y, bubbles: bubbles, cancelable: true,
        pointerType: 'mouse', isPrimary: true, button: 0, buttons: pressed ? 1 : 0
      });
    }
    function setHover(next, x, y) {
      if (next === hovered) return;
      var before = hovered ? chainOf(hovered) : [], after = next ? chainOf(next) : [];
      if (hovered) {
        before.forEach(function (n) {
          if (after.indexOf(n) < 0) n.dispatchEvent(pointerEvent('pointerleave', x, y, false));
        });
        hovered.dispatchEvent(pointerEvent('pointerout', x, y, true));
      }
      if (next) {
        next.dispatchEvent(pointerEvent('pointerover', x, y, true));
        after.slice().reverse().forEach(function (n) {
          if (before.indexOf(n) < 0) n.dispatchEvent(pointerEvent('pointerenter', x, y, false));
        });
      }
      hovered = next;
    }
    function moveTo(x, y) {
      var el = targetAt(x, y);
      setHover(el, x, y);
      var receiver = pressed || el;
      if (receiver) receiver.dispatchEvent(pointerEvent('pointermove', x, y, true));
    }
    function pressAt(x, y) {
      var el = targetAt(x, y);
      setHover(el, x, y);
      if (!el) return;
      pressed = el;
      el.dispatchEvent(pointerEvent('pointerdown', x, y, true));
    }
    function releaseAt(x, y) {
      var start = pressed, el = targetAt(x, y);
      pressed = null;
      setHover(el, x, y);
      if (!start) return;
      start.dispatchEvent(pointerEvent('pointerup', x, y, true));
      var shared = null, others = el ? chainOf(el) : [];
      chainOf(start).some(function (n) { if (others.indexOf(n) >= 0) { shared = n; return true; } return false; });
      if (shared) shared.dispatchEvent(new MouseEvent('click', { clientX: x, clientY: y, bubbles: true, cancelable: true, button: 0 }));
    }
    // document.currentScript can be null for SVG scripts in WebViews. The
    // bundled runtime may therefore be loaded without its auto-bootstrap.
    if (!svg.splinely && window.SplinelyRuntime) {
      window.SplinelyRuntime.createRuntime(svg);
    }
    host.reply({ locora: 1, runtimeReady: !!svg.splinely });
    host.onMessage(function (d) {
      if (!d || d.locora !== 1) return;
      if (d.checkRuntime) {
        host.reply({ locora: 1, runtimeReady: !!svg.splinely });
        return;
      }
      if (!svg.splinely) return;
      if (d.still) {
        // A still icon keeps its first frame: nothing plays, so nothing is drawn on every frame.
        var stillClips = (svg.splinely.sourceProject && svg.splinely.sourceProject.clips) || [];
        stillClips.forEach(function (clip) { svg.splinely.clip(clip.id).pause(); });
        svg.splinely.timeline.pause();
        return;
      }
      if (d.timed && typeof d.timed.action === 'string') {
        try {
          var runtime = svg.splinely, duration = animationDuration();
          if (!Number.isFinite(duration) || duration <= 0) {
            timedReply('timedError', 'Diese Splinely-Animation hat keine Zeitleiste');
            return;
          }
          if (d.timed.action === 'init') {
            timedClips = runtime.sourceProject.clips.filter(function (clip) {
              return clip.autoplay || runtime.clip(clip.id).playing;
            }).map(function (clip) { return clip.id; });
            if (!timedClips.length) {
              timedClips = runtime.sourceProject.clips.map(function (clip) { return clip.id; });
            }
            if (!timedClips.length) {
              timedReply('timedError', 'Keine Splinely-Animation vorhanden');
              return;
            }
            timed = true;
            pauseTimed();
            timedClips.forEach(function (id) { runtime.clip(id).seek(0); });
            runtime.timeline.seek(0);
            timedReply('timedReady', duration);
          } else if (timed && (d.timed.action === 'seek' || d.timed.action === 'playTo')) {
            var target = d.timed.time;
            if (!Number.isFinite(target) || target < 0 || target > duration) {
              timedReply('timedError', 'Zeit liegt außerhalb der Animation');
              return;
            }
            pauseTimed();
            if (d.timed.action === 'playTo' && Number.isFinite(d.timed.start)
                && d.timed.start >= 0 && d.timed.start <= target) {
              runtime.timeline.seek(d.timed.start);
            }
            if (d.timed.action === 'seek' || target <= runtime.timeline.time) {
              runtime.timeline.seek(target);
              timedReply('timedAt', target);
            } else {
              timedCancel = runtime.timeline.at(target, function () {
                pauseTimed();
                runtime.timeline.seek(target);
                timedReply('timedAt', target);
              });
              timedClips.forEach(function (id) { runtime.clip(id).play(); });
              timedTicker = setInterval(function () {
                timedReply('timedPosition', runtime.timeline.time);
              }, 100);
              runtime.timeline.play();
            }
          }
        } catch (err) {
          timedReply('timedError', String(err));
        }
        return;
      }
      if (d.pointer && Number.isFinite(d.pointer.x) && Number.isFinite(d.pointer.y)) {
        // Pointer followers read the root; hover and drags use the element underneath.
        svg.dispatchEvent(new PointerEvent('pointermove', {
          clientX: d.pointer.x, clientY: d.pointer.y, bubbles: true,
          pointerType: 'mouse', isPrimary: true
        }));
        moveTo(d.pointer.x, d.pointer.y);
        return;
      }
      if (d.pointerDown && Number.isFinite(d.pointerDown.x) && Number.isFinite(d.pointerDown.y)) {
        pressAt(d.pointerDown.x, d.pointerDown.y);
        return;
      }
      if (d.pointerUp && Number.isFinite(d.pointerUp.x) && Number.isFinite(d.pointerUp.y)) {
        releaseAt(d.pointerUp.x, d.pointerUp.y);
        return;
      }
      if (d.pointerLeave) {
        pressed = null;
        setHover(null, 0, 0);
        svg.dispatchEvent(new PointerEvent('pointerleave', { pointerType: 'mouse' }));
        return;
      }
      try {
        if (typeof d.machine === 'string' && typeof d.send === 'string') {
          svg.splinely.machine(d.machine).send(d.send);
        }
      } catch (err) {
        svg.splinely.reportError && svg.splinely.reportError(err);
      }
    });
    return function dispose() {
    pauseTimed();
  };
}
