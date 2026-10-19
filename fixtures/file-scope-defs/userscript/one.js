(function() {
    'use strict';
    window.Mira = window.Mira || {};
    function issueToken(href) { return href.split('/').pop(); }
    function display() { return issueToken(window.location.href); }
    window.Mira.Panel = { display };
    display();
})();
