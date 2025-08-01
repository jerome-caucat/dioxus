// A unified interface for setting attributes on a node

// this function should try and stay fast, if possible
export function setAttributeInner(
  node: HTMLElement,
  field: string,
  value: string,
  ns: string
) {
  // we support a single namespace by default: style
  if (ns === "style") {
    node.style.setProperty(field, value);
    return;
  }

  // If there's a namespace, use setAttributeNS (svg, mathml, etc.)
  if (!!ns) {
    node.setAttributeNS(ns, field, value);
    return;
  }

  // A few attributes are need to be set with either boolean values or require some sort of translation
  switch (field) {
    case "value":
      // If this is a option element, set the attribute normally
      if (node.tagName === "OPTION") {
        setAttributeDefault(node, field, value);
      } else {
        // Otherwise, set the property if it has changed
        // @ts-ignore
        if (node.value !== value) {
          // @ts-ignore
          node.value = value;
        }
      }
      break;

    case "initial_value":
      // @ts-ignore
      node.defaultValue = value;
      break;

    case "checked":
      // @ts-ignore
      node.checked = truthy(value);
      break;

    case "initial_checked":
      // @ts-ignore
      node.defaultChecked = truthy(value);
      break;

    case "selected":
      // @ts-ignore
      node.selected = truthy(value);
      break;

    case "initial_selected":
      // @ts-ignore
      node.defaultSelected = truthy(value);
      break;

    case "dangerous_inner_html":
      node.innerHTML = value;
      break;

    case "style":
      // Save the existing styles
      const existingStyles: Record<string, string> = {};

      for (let i = 0; i < node.style.length; i++) {
        const prop = node.style[i];
        existingStyles[prop] = node.style.getPropertyValue(prop);
      }
      // Override all styles
      node.setAttribute(field, value);
      // Restore the old styles
      for (const prop in existingStyles) {
        // If it wasn't overridden, restore it
        if (!node.style.getPropertyValue(prop)) {
          node.style.setProperty(prop, existingStyles[prop]);
        }
      }
      break;

    case "multiple":
      setAttributeDefault(node, field, value);
      // reset the selected value whenever multiple changes
      // @ts-ignore
      if (node.options !== null && node.options !== undefined) {
        // @ts-ignore
        let options = node.options;
        for (const option of options) {
          option.selected = option.defaultSelected;
        }
      }
      break;

    default:
      setAttributeDefault(node, field, value);
  }
}

function setAttributeDefault(node: HTMLElement, field: string, value: string) {
  // The presence of a an attribute is enough to set it to true, provided the value is being set to a truthy value
  // Again, kinda ugly and would prefer this logic to be baked into dioxus-html at compile time
  // https://github.com/facebook/react/blob/8b88ac2592c5f555f315f9440cbb665dd1e7457a/packages/react-dom/src/shared/DOMProperty.js#L352-L364
  if (!truthy(value) && isBoolAttr(field)) {
    node.removeAttribute(field);
  } else {
    node.setAttribute(field, value);
  }
}

function truthy(val: string | boolean) {
  return val === "true" || val === true;
}

function isBoolAttr(field: string): boolean {
  switch (field) {
    case "allowfullscreen":
    case "allowpaymentrequest":
    case "async":
    case "autofocus":
    case "autoplay":
    case "checked":
    case "controls":
    case "default":
    case "defer":
    case "disabled":
    case "formnovalidate":
    case "hidden":
    case "ismap":
    case "itemscope":
    case "loop":
    case "multiple":
    case "muted":
    case "nomodule":
    case "novalidate":
    case "open":
    case "playsinline":
    case "readonly":
    case "required":
    case "reversed":
    case "selected":
    case "truespeed":
    case "webkitdirectory":
      return true;
    default:
      return false;
  }
}
