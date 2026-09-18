// Client-side search + category filter for the public catalog page
// (templates/catalog.html). All cards are rendered server-side up front;
// this just toggles visibility, the same "no build step, one plain
// script" approach as workflow_builder.js -- there's no dynamic data here
// that would justify a round trip to the server for every keystroke.
(function () {
  const searchInput = document.getElementById("catalog-search");
  const chipRow = document.getElementById("catalog-chips");
  const emptyState = document.getElementById("catalog-empty");
  const sections = Array.from(document.querySelectorAll(".catalog-section"));
  if (!searchInput || !chipRow) return;

  let activeCategory = "";

  function apply() {
    const query = searchInput.value.trim().toLowerCase();
    let visibleCount = 0;

    for (const section of sections) {
      const cards = Array.from(section.querySelectorAll(".catalog-card"));
      let visibleInSection = 0;
      for (const card of cards) {
        const matchesQuery = query === "" || card.dataset.search.includes(query);
        const matchesCategory = activeCategory === "" || card.dataset.category === activeCategory;
        const visible = matchesQuery && matchesCategory;
        card.hidden = !visible;
        if (visible) visibleInSection++;
      }
      section.hidden = visibleInSection === 0;
      visibleCount += visibleInSection;
    }

    emptyState.hidden = visibleCount !== 0;
  }

  searchInput.addEventListener("input", apply);

  chipRow.addEventListener("click", (event) => {
    const chip = event.target.closest(".chip");
    if (!chip) return;
    activeCategory = chip.dataset.category;
    for (const other of chipRow.querySelectorAll(".chip")) {
      other.classList.toggle("is-active", other === chip);
    }
    apply();
  });
})();
