/* ============================================
   QINDOWS — Main JavaScript
   Particle Mesh · Scroll Reveals · Interactions
   ============================================ */

(function () {
  'use strict';

  // ─── Particle Mesh Background ─────────────────────────────────
  const canvas = document.getElementById('mesh-canvas');
  const ctx = canvas.getContext('2d');
  let particles = [];
  let mouse = { x: -1000, y: -1000 };
  let animFrame;

  const PARTICLE_CONFIG = {
    count: 80,
    maxDist: 160,
    speed: 0.3,
    mouseRadius: 200,
    baseAlpha: 0.25,
    lineAlpha: 0.07,
    colors: ['#06d6a0', '#118ab2', '#7b2ff7', '#ef476f', '#ffd166'],
  };

  function resizeCanvas() {
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
  }

  function createParticles() {
    particles = [];
    const count = window.innerWidth < 768
      ? Math.floor(PARTICLE_CONFIG.count * 0.5)
      : PARTICLE_CONFIG.count;

    for (let i = 0; i < count; i++) {
      particles.push({
        x: Math.random() * canvas.width,
        y: Math.random() * canvas.height,
        vx: (Math.random() - 0.5) * PARTICLE_CONFIG.speed,
        vy: (Math.random() - 0.5) * PARTICLE_CONFIG.speed,
        r: Math.random() * 1.8 + 0.5,
        color: PARTICLE_CONFIG.colors[Math.floor(Math.random() * PARTICLE_CONFIG.colors.length)],
      });
    }
  }

  function drawParticles() {
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    // Draw connections
    for (let i = 0; i < particles.length; i++) {
      for (let j = i + 1; j < particles.length; j++) {
        const dx = particles[i].x - particles[j].x;
        const dy = particles[i].y - particles[j].y;
        const dist = Math.sqrt(dx * dx + dy * dy);

        if (dist < PARTICLE_CONFIG.maxDist) {
          const alpha = (1 - dist / PARTICLE_CONFIG.maxDist) * PARTICLE_CONFIG.lineAlpha;
          ctx.beginPath();
          ctx.moveTo(particles[i].x, particles[i].y);
          ctx.lineTo(particles[j].x, particles[j].y);
          ctx.strokeStyle = `rgba(123, 47, 247, ${alpha})`;
          ctx.lineWidth = 0.5;
          ctx.stroke();
        }
      }
    }

    // Draw and update particles
    for (const p of particles) {
      // Mouse repulsion
      const mdx = p.x - mouse.x;
      const mdy = p.y - mouse.y;
      const mDist = Math.sqrt(mdx * mdx + mdy * mdy);
      if (mDist < PARTICLE_CONFIG.mouseRadius && mDist > 0) {
        const force = (1 - mDist / PARTICLE_CONFIG.mouseRadius) * 0.015;
        p.vx += (mdx / mDist) * force;
        p.vy += (mdy / mDist) * force;
      }

      p.x += p.vx;
      p.y += p.vy;

      // Damping
      p.vx *= 0.999;
      p.vy *= 0.999;

      // Boundary wrap
      if (p.x < -20) p.x = canvas.width + 20;
      if (p.x > canvas.width + 20) p.x = -20;
      if (p.y < -20) p.y = canvas.height + 20;
      if (p.y > canvas.height + 20) p.y = -20;

      // Draw particle
      ctx.beginPath();
      ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
      ctx.fillStyle = p.color;
      ctx.globalAlpha = PARTICLE_CONFIG.baseAlpha;
      ctx.fill();
      ctx.globalAlpha = 1;
    }

    animFrame = requestAnimationFrame(drawParticles);
  }

  // Listen for mouse
  document.addEventListener('mousemove', (e) => {
    mouse.x = e.clientX;
    mouse.y = e.clientY;
  });

  document.addEventListener('mouseleave', () => {
    mouse.x = -1000;
    mouse.y = -1000;
  });

  window.addEventListener('resize', () => {
    resizeCanvas();
    createParticles();
  });

  resizeCanvas();
  createParticles();
  drawParticles();

  // ─── Navigation ───────────────────────────────────────────────
  const nav = document.getElementById('main-nav');
  const navToggle = document.getElementById('nav-toggle');
  const navLinks = document.getElementById('nav-links');

  window.addEventListener('scroll', () => {
    nav.classList.toggle('scrolled', window.scrollY > 40);
  });

  navToggle.addEventListener('click', () => {
    navLinks.classList.toggle('open');
  });

  // Close mobile menu on link click
  navLinks.querySelectorAll('.nav-link').forEach((link) => {
    link.addEventListener('click', () => {
      navLinks.classList.remove('open');
    });
  });

  // ─── Scroll Reveal ────────────────────────────────────────────
  const revealObserver = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add('visible');
          revealObserver.unobserve(entry.target);
        }
      });
    },
    { threshold: 0.1, rootMargin: '0px 0px -40px 0px' }
  );

  document.querySelectorAll('.reveal').forEach((el) => {
    revealObserver.observe(el);
  });

  // ─── Performance Bar Animations ───────────────────────────────
  const perfObserver = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          const bars = entry.target.querySelectorAll('.perf-bar-fill');
          bars.forEach((bar) => {
            const width = bar.dataset.width;
            bar.style.setProperty('--bar-width', `${width}%`);
            setTimeout(() => bar.classList.add('animated'), 200);
          });
          perfObserver.unobserve(entry.target);
        }
      });
    },
    { threshold: 0.3 }
  );

  const perfTable = document.querySelector('.perf-table');
  if (perfTable) perfObserver.observe(perfTable);

  // ─── Genesis Terminal Animation ───────────────────────────────
  const genesisObserver = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          const lines = entry.target.querySelectorAll('.terminal-line');
          lines.forEach((line) => {
            const delay = parseInt(line.dataset.delay, 10) || 0;
            setTimeout(() => line.classList.add('visible'), delay);
          });
          genesisObserver.unobserve(entry.target);
        }
      });
    },
    { threshold: 0.5 }
  );

  const genesisTerminal = document.querySelector('.genesis-terminal');
  if (genesisTerminal) genesisObserver.observe(genesisTerminal);

  // ─── Smooth Scroll for Anchor Links ───────────────────────────
  document.querySelectorAll('a[href^="#"]').forEach((anchor) => {
    anchor.addEventListener('click', function (e) {
      e.preventDefault();
      const target = document.querySelector(this.getAttribute('href'));
      if (target) {
        const offset = 80; // nav height
        const top = target.getBoundingClientRect().top + window.scrollY - offset;
        window.scrollTo({ top, behavior: 'smooth' });
      }
    });
  });

  // ─── Active Nav Link Highlight ────────────────────────────────
  const sections = document.querySelectorAll('section[id]');
  const navLinksAll = document.querySelectorAll('.nav-link');

  const activeLinkObserver = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          const id = entry.target.id;
          navLinksAll.forEach((link) => {
            link.style.color = link.getAttribute('href') === `#${id}`
              ? 'var(--text-primary)'
              : '';
          });
        }
      });
    },
    { threshold: 0.3, rootMargin: '-80px 0px -60% 0px' }
  );

  sections.forEach((section) => activeLinkObserver.observe(section));

  // ─── Registration Modal ───────────────────────────────────────
  const regModal = document.getElementById('reg-modal');
  const regClose = document.getElementById('reg-close');
  const regBackdrop = regModal ? regModal.querySelector('.reg-backdrop') : null;
  const progressFill = document.getElementById('reg-progress-fill');
  const step1 = document.getElementById('reg-step-1');
  const step2 = document.getElementById('reg-step-2');
  const step3 = document.getElementById('reg-step-3');
  const regName = document.getElementById('reg-name');
  const regContact = document.getElementById('reg-contact');
  const regInterest = document.getElementById('reg-interest');
  const regNext1 = document.getElementById('reg-next-1');
  const regVerify = document.getElementById('reg-verify');
  const regDone = document.getElementById('reg-done');
  const otpDesc = document.getElementById('otp-desc');
  const otpInputs = document.querySelectorAll('.otp-digit');
  const regVerifying = document.getElementById('reg-verifying');
  const otpResend = document.getElementById('otp-resend');

  let selectedUsage = new Set();

  function openModal() {
    if (!regModal) return;
    regModal.classList.add('open');
    regModal.setAttribute('aria-hidden', 'false');
    document.body.style.overflow = 'hidden';
  }

  function closeModal() {
    if (!regModal) return;
    regModal.classList.remove('open');
    regModal.setAttribute('aria-hidden', 'true');
    document.body.style.overflow = '';
  }

  // Open modal from any Start button
  document.querySelectorAll('#hero-start-btn, #genesis-start-btn').forEach((btn) => {
    btn.addEventListener('click', openModal);
  });

  // Close modal
  if (regClose) regClose.addEventListener('click', closeModal);
  if (regBackdrop) regBackdrop.addEventListener('click', closeModal);
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && regModal && regModal.classList.contains('open')) closeModal();
  });

  // Device card selection (multi-select toggle)
  document.querySelectorAll('.reg-card').forEach((card) => {
    card.addEventListener('click', () => {
      const val = card.dataset.value;
      card.classList.toggle('selected');
      if (card.classList.contains('selected')) {
        selectedUsage.add(val);
      } else {
        selectedUsage.delete(val);
      }
      validateStep1();
    });
  });

  // Step 1 validation
  function validateStep1() {
    const nameOk = regName && regName.value.trim().length > 0;
    const contactOk = regContact && regContact.value.trim().length > 0;
    const usageOk = selectedUsage.size > 0;
    if (regNext1) regNext1.disabled = !(nameOk && contactOk && usageOk);
  }

  if (regName) regName.addEventListener('input', validateStep1);
  if (regContact) regContact.addEventListener('input', validateStep1);

  // Detect email vs phone
  function isEmail(str) {
    return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(str);
  }

  function isPhone(str) {
    return /^[\+]?[\d\s\-\(\)]{7,}$/.test(str.replace(/\s/g, ''));
  }

  // Step 1 → Step 2
  if (regNext1) {
    regNext1.addEventListener('click', () => {
      const contact = regContact.value.trim();
      let contactType = 'contact';
      if (isEmail(contact)) {
        contactType = 'email';
        otpDesc.textContent = `We've sent a 6-digit verification code to ${contact}`;
      } else if (isPhone(contact)) {
        contactType = 'phone';
        otpDesc.textContent = `We've sent a 6-digit verification code via SMS to ${contact}`;
      } else {
        otpDesc.textContent = `We've sent a 6-digit verification code to ${contact}`;
      }

      // Switch to step 2
      step1.classList.add('reg-step-hidden');
      step2.classList.remove('reg-step-hidden');
      progressFill.style.width = '66%';

      // Focus first OTP digit
      setTimeout(() => {
        if (otpInputs[0]) otpInputs[0].focus();
      }, 300);
    });
  }

  // OTP digit handling
  otpInputs.forEach((input, idx) => {
    input.addEventListener('input', (e) => {
      const val = e.target.value.replace(/\D/g, '');
      e.target.value = val.slice(0, 1);
      if (val && idx < otpInputs.length - 1) {
        otpInputs[idx + 1].focus();
      }
      validateOTP();
    });

    input.addEventListener('keydown', (e) => {
      if (e.key === 'Backspace' && !e.target.value && idx > 0) {
        // Skip the separator by checking if prev is an input
        const prevIdx = idx > 3 ? idx - 1 : (idx === 3 ? 2 : idx - 1);
        otpInputs[prevIdx].focus();
      }
    });

    // Paste support
    input.addEventListener('paste', (e) => {
      e.preventDefault();
      const pasted = (e.clipboardData || window.clipboardData).getData('text').replace(/\D/g, '');
      for (let i = 0; i < Math.min(pasted.length, 6); i++) {
        otpInputs[i].value = pasted[i];
      }
      if (pasted.length >= 6) {
        otpInputs[5].focus();
      }
      validateOTP();
    });
  });

  function validateOTP() {
    const code = Array.from(otpInputs).map((i) => i.value).join('');
    if (regVerify) regVerify.disabled = code.length < 6;
  }

  // Step 2 → Step 3 (verification)
  if (regVerify) {
    regVerify.addEventListener('click', () => {
      regVerify.style.display = 'none';
      regVerifying.style.display = 'flex';

      // Simulate verification (2s delay)
      setTimeout(() => {
        // Store data
        const formData = {
          name: regName.value.trim(),
          contact: regContact.value.trim(),
          usage: Array.from(selectedUsage),
          interest: regInterest ? regInterest.value.trim() : '',
          registeredAt: new Date().toISOString(),
        };
        try {
          localStorage.setItem('qindows_registration', JSON.stringify(formData));
        } catch (e) { /* silently fail */ }

        // Populate success
        const successName = document.getElementById('success-name');
        const successContact = document.getElementById('success-contact');
        const successNode = document.getElementById('success-node-id');
        if (successName) successName.textContent = formData.name;
        if (successContact) successContact.textContent = formData.contact;
        if (successNode) {
          // Generate a pseudo Node ID
          const nodeId = 'Q-' + Math.random().toString(36).substring(2, 6).toUpperCase() +
            '-' + Math.random().toString(36).substring(2, 6).toUpperCase() +
            '-' + Math.random().toString(36).substring(2, 6).toUpperCase();
          successNode.textContent = nodeId;
        }

        // Switch to step 3
        step2.classList.add('reg-step-hidden');
        step3.classList.remove('reg-step-hidden');
        progressFill.style.width = '100%';
      }, 2000);
    });
  }

  // Resend OTP
  if (otpResend) {
    otpResend.addEventListener('click', () => {
      otpResend.textContent = 'Code resent!';
      otpResend.disabled = true;
      setTimeout(() => {
        otpResend.textContent = 'Resend code';
        otpResend.disabled = false;
      }, 3000);
    });
  }

  // Step 3 → Close
  if (regDone) {
    regDone.addEventListener('click', () => {
      closeModal();
      // Reset modal state for potential re-open
      setTimeout(() => {
        step1.classList.remove('reg-step-hidden');
        step2.classList.add('reg-step-hidden');
        step3.classList.add('reg-step-hidden');
        progressFill.style.width = '33%';
        regVerify.style.display = '';
        regVerifying.style.display = 'none';
        otpInputs.forEach((i) => (i.value = ''));
      }, 500);
    });
  }

})();
