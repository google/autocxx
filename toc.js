// Populate the sidebar
//
// This is a script, and not included directly in the page, to control the total size of the book.
// The TOC contains an entry for each page, so if each page includes a copy of the TOC,
// the total size of the page becomes O(n**2).
class MDBookSidebarScrollbox extends HTMLElement {
    constructor() {
        super();
    }
    connectedCallback() {
        this.innerHTML = '<ol class="chapter"><li class="chapter-item expanded "><a href="index.html"><strong aria-hidden="true">1.</strong> Rust ❤️  pre-existing C++</a></li><li class="chapter-item expanded "><a href="tutorial.html"><strong aria-hidden="true">2.</strong> Tutorial</a></li><li class="chapter-item expanded "><a href="workflow.html"><strong aria-hidden="true">3.</strong> Workflow</a></li><li class="chapter-item expanded "><a href="allowlist.html"><strong aria-hidden="true">4.</strong> Allowlist and syntax</a></li><li class="chapter-item expanded "><a href="building.html"><strong aria-hidden="true">5.</strong> Building</a></li><li class="chapter-item expanded "><a href="cpp_types.html"><strong aria-hidden="true">6.</strong> C++ structs, enums and classes</a></li><li class="chapter-item expanded "><a href="references_etc.html"><strong aria-hidden="true">7.</strong> Pointers, references, values</a></li><li class="chapter-item expanded "><a href="storage.html"><strong aria-hidden="true">8.</strong> Storage - stack and heaps</a></li><li class="chapter-item expanded "><a href="primitives.html"><strong aria-hidden="true">9.</strong> Built-in types</a></li><li class="chapter-item expanded "><a href="naming.html"><strong aria-hidden="true">10.</strong> C++ type and function names</a></li><li class="chapter-item expanded "><a href="cpp_functions.html"><strong aria-hidden="true">11.</strong> C++ functions</a></li><li class="chapter-item expanded "><a href="rust_calls.html"><strong aria-hidden="true">12.</strong> Callbacks into Rust</a></li><li class="chapter-item expanded "><a href="other_features.html"><strong aria-hidden="true">13.</strong> Other C++ features</a></li><li class="chapter-item expanded "><a href="safety.html"><strong aria-hidden="true">14.</strong> Safety</a></li><li class="chapter-item expanded "><a href="rustic.html"><strong aria-hidden="true">15.</strong> Rustic bindings</a></li><li class="chapter-item expanded "><a href="large_codebase.html"><strong aria-hidden="true">16.</strong> Large codebases</a></li><li class="chapter-item expanded "><a href="examples.html"><strong aria-hidden="true">17.</strong> Examples</a></li><li class="chapter-item expanded "><a href="credits.html"><strong aria-hidden="true">18.</strong> Credits</a></li><li class="chapter-item expanded "><a href="contributing.html"><strong aria-hidden="true">19.</strong> Contributing</a></li><li class="chapter-item expanded "><a href="code-of-conduct.html"><strong aria-hidden="true">20.</strong> Code of Conduct</a></li></ol>';
        // Set the current, active page, and reveal it if it's hidden
        let current_page = document.location.href.toString().split("#")[0];
        if (current_page.endsWith("/")) {
            current_page += "index.html";
        }
        var links = Array.prototype.slice.call(this.querySelectorAll("a"));
        var l = links.length;
        for (var i = 0; i < l; ++i) {
            var link = links[i];
            var href = link.getAttribute("href");
            if (href && !href.startsWith("#") && !/^(?:[a-z+]+:)?\/\//.test(href)) {
                link.href = path_to_root + href;
            }
            // The "index" page is supposed to alias the first chapter in the book.
            if (link.href === current_page || (i === 0 && path_to_root === "" && current_page.endsWith("/index.html"))) {
                link.classList.add("active");
                var parent = link.parentElement;
                if (parent && parent.classList.contains("chapter-item")) {
                    parent.classList.add("expanded");
                }
                while (parent) {
                    if (parent.tagName === "LI" && parent.previousElementSibling) {
                        if (parent.previousElementSibling.classList.contains("chapter-item")) {
                            parent.previousElementSibling.classList.add("expanded");
                        }
                    }
                    parent = parent.parentElement;
                }
            }
        }
        // Track and set sidebar scroll position
        this.addEventListener('click', function(e) {
            if (e.target.tagName === 'A') {
                sessionStorage.setItem('sidebar-scroll', this.scrollTop);
            }
        }, { passive: true });
        var sidebarScrollTop = sessionStorage.getItem('sidebar-scroll');
        sessionStorage.removeItem('sidebar-scroll');
        if (sidebarScrollTop) {
            // preserve sidebar scroll position when navigating via links within sidebar
            this.scrollTop = sidebarScrollTop;
        } else {
            // scroll sidebar to current active section when navigating via "next/previous chapter" buttons
            var activeSection = document.querySelector('#sidebar .active');
            if (activeSection) {
                activeSection.scrollIntoView({ block: 'center' });
            }
        }
        // Toggle buttons
        var sidebarAnchorToggles = document.querySelectorAll('#sidebar a.toggle');
        function toggleSection(ev) {
            ev.currentTarget.parentElement.classList.toggle('expanded');
        }
        Array.from(sidebarAnchorToggles).forEach(function (el) {
            el.addEventListener('click', toggleSection);
        });
    }
}
window.customElements.define("mdbook-sidebar-scrollbox", MDBookSidebarScrollbox);
