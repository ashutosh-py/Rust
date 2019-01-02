// From rust:
/* global resourcesSuffix */

// eslint doesn't understand the functions here are used elsewhere
/* eslint no-unused-vars: "off" */

var currentTheme = document.getElementById("themeStyle");
var mainTheme = document.getElementById("mainThemeStyle");

var savedHref = [];

function hasClass(elem, className) {
    return elem && elem.classList && elem.classList.contains(className);
}

function addClass(elem, className) {
    if (!elem || !elem.classList) {
        return;
    }
    elem.classList.add(className);
}

function removeClass(elem, className) {
    if (!elem || !elem.classList) {
        return;
    }
    elem.classList.remove(className);
}

function isHidden(elem) {
    return elem.offsetParent === null;
}

function onEach(arr, func, reversed) {
    if (arr && arr.length > 0 && func) {
        let length = arr.length;
        if (reversed !== true) {
            for (let i = 0; i < length; ++i) {
                if (func(arr[i]) === true) {
                    return true;
                }
            }
        } else {
            for (let i = length - 1; i >= 0; --i) {
                if (func(arr[i]) === true) {
                    return true;
                }
            }
        }
    }
    return false;
}

function onEachLazy(lazyArray, func, reversed) {
    return onEach(
        Array.prototype.slice.call(lazyArray),
        func,
        reversed);
}

function usableLocalStorage() {
    // Check if the browser supports localStorage at all:
    if (typeof(Storage) === "undefined") {
        return false;
    }
    // Check if we can access it; this access will fail if the browser
    // preferences deny access to localStorage, e.g., to prevent storage of
    // "cookies" (or cookie-likes, as is the case here).
    try {
        window.localStorage;
    } catch(err) {
        // Storage is supported, but browser preferences deny access to it.
        return false;
    }

    return true;
}

function updateLocalStorage(name, value) {
    if (usableLocalStorage()) {
        localStorage[name] = value;
    } else {
        // No Web Storage support so we do nothing
    }
}

function getCurrentValue(name) {
    if (usableLocalStorage() && localStorage[name] !== undefined) {
        return localStorage[name];
    }
    return null;
}

function switchTheme(styleElem, mainStyleElem, newTheme) {
    let fullBasicCss = "rustdoc" + resourcesSuffix + ".css";
    let fullNewTheme = newTheme + resourcesSuffix + ".css";
    let newHref = mainStyleElem.href.replace(fullBasicCss, fullNewTheme);

    if (styleElem.href === newHref) {
        return;
    }

    let found = false;
    if (savedHref.length === 0) {
        onEachLazy(document.getElementsByTagName("link"), function(el) {
            savedHref.push(el.href);
        });
    }
    onEach(savedHref, function(el) {
        if (el === newHref) {
            found = true;
            return true;
        }
    });
    if (found === true) {
        styleElem.href = newHref;
        updateLocalStorage("rustdoc-theme", newTheme);
    }
}

switchTheme(currentTheme, mainTheme, getCurrentValue("rustdoc-theme") || "light");
