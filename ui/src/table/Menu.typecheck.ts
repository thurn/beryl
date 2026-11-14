import type { Row } from "./Menu";

export const oneLevel: Row = {
  id: "parent",
  label: "Parent",
  submenu: [{ id: "child", label: "Child" }],
};

export const twoLevels: Row = {
  id: "parent",
  label: "Parent",
  submenu: [
    {
      id: "child",
      label: "Child",
      // @ts-expect-error A submenu row cannot have a submenu.
      submenu: [{ id: "grandchild", label: "Grandchild" }],
    },
  ],
};
