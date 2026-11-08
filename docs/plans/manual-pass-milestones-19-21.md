# Manual pass: milestones 19, 20 and 21

One sitting for every manual check in milestones 19 ([servers and channels](servers-and-channels.md)), 20 ([groups and DMs](groups-and-dms.md)) and 21 ([server list and accounts](server-list-and-accounts.md)). It merges the "Remaining Manual Steps" of the three implementation records, renumbered, with one cast of accounts, and with the two-account and theme checks moved to the end so they're done once.

## Before you start
1. **Commit the chunks.** Run `commit-chunks.sh` from the task's artifacts (`.humanlayer/tasks/let-s-work-through-the-next-3-milestones-e3d7/`). It makes one signed commit per chunk (Touch ID each time) and stops if the staged changes no longer match the last chunk.
2. **Run the app bundled** (`scripts/run-bundled.sh`), so banners and the Dock badge behave as in daily use.
3. **The cast,** all on `api.stoat.chat` and all friends with each other:
   - **A**: you, signed in to RuSToat. Also signed in to the web client, in a browser, to watch.
   - **B**: a friend, in the web client in a second browser (or a private window).
   - **C**: a friend, signed in to RuSToat as a second account.
   - **D**: any third account signed in to RuSToat, for dragging accounts (step 87).
   - A needs at least five servers by Part 3 (the test server from step 5 counts), some with icons and one without, and one shared with B.

Write down anything that fails with its step number. When everything passes, the Definition of Done checklists in the three implementation records can be ticked and the sign-off recorded, as for earlier milestones.

## Part 1: Servers and channels (milestone 19)

**Create a server**
1. As A, click the rail's "+": a menu with "Create a server…" (plus icon) and "Join a server…" opens to the right of the rail, not clipped by the window's edge. Try again with the window short enough that the "+" is near the bottom.
2. Choose Join a server…: "Join a server" opens. Close it.
3. Choose Create a server…: "Create a server" opens with the field focused. Create is disabled while the name is empty or blank and past 32 characters; the counter shows the count and turns red past 32.
4. The "By creating this server, you agree to the Acceptable Use Policy." line shows, and its link opens `https://stoat.chat/aup` in the browser.
5. Name it "M19 Test" and Create: it appears at the end of the rail, General opens, and you can send a message there. No toast.
6. Stoat → Create Server… opens the same dialog. Close it. With the login view on screen (adding an account, say), Stoat → Create Server… does nothing.

**Categories and channels**
7. Right-click the channel list's empty space below General: only "Create channel…" and "Create category…". Right-click General: Create invite…, Create channel…, Create category…, Delete channel… (red), a separator, Copy channel ID; the list's own menu doesn't also open.
8. Create category… "Alpha": "Create a new category", Create disabled until the name is valid. Alpha appears at the bottom as a heading with no rows. Create "Beta" and "Gamma" the same way. Fold and unfold Gamma (empty): it toggles.
9. Right-click Alpha's heading: Create channel…, Create category…, Rename category…, Delete category… (red), a separator, Copy category ID. Each icon draws (none blank).
10. From Alpha's menu, Create channel…: "Create channel", "in ALPHA", Text Channel chosen; the radios switch, and Create follows the same name rules as step 3. Create a text channel "a-text": it lands at the end of Alpha and opens.
11. From Beta's menu, create a text channel "b-text". Then right-click b-text → Create channel…, and create a Voice Channel "b-voice": it lands at the end of Beta with the speaker icon and opens.
12. From the empty space's menu, create "loose": it's uncategorized, at the bottom of the top group, and opens.
13. The web client shows a-text, b-text, b-voice and loose in the same places.
14. Fold Beta, then Rename category… on it: "Rename category" with "Beta" filled in. Rename it "Bravo": the heading changes at once (and in the web client), and it stays folded.
15. Open Create channel… from Gamma's menu and leave it open; delete Gamma in the web client; then Create "orphan": the channel is made, uncategorized, with no toast.
16. Open Rename category… on Alpha and leave it open; delete Alpha in the web client; then Rename: the error line says the category has been deleted. Close it. (Alpha's channels are now uncategorized.)
17. Create a category "Delta" in RuSToat. Open Rename on it, rename it in the web client to "Web", then save "Mine" in RuSToat: "Mine" wins, here and in the web client.
18. Fold Bravo and Delete category… on it: "Delete BRAVO?" with "Its channels won't be deleted. They'll move out of the category." Delete shows a spinner; then b-text and b-voice move to the top group, none deleted, and the web client agrees. Create a category "Echo": it isn't folded.
19. Delete every remaining category, the last one too: everything is uncategorized, with no error.

**Server and channel info**
20. In the web client, give M19 Test an icon, a banner and a Markdown description with **bold** text, an external link, an invite link and a channel mention. Give General a two-line description whose first line is "Rules".
21. Hover the server's name above the channel list: a pointer, the hover colours and "Server info". Click it: the banner across the top, the icon beside "M19 Test", and the description rendered and selectable. Close, Escape and clicking outside each close it.
22. In Server info, click the external link (a warning asks first), the invite link (Join a server opens) and the channel mention (the channel opens).
23. With Server info open, change the description in the web client: it updates. Remove the banner, icon and description: no banner, the initials "MT", and "No description." in muted text.
24. Home's "Direct Messages" title isn't clickable.
25. Open General: "Rules" shows after its name in the header, muted, with "Click to show full description" on hover. Click the line, then the name: Channel info shows the `#` beside "General" and the whole description. b-voice's info shows the speaker. A channel without a description shows only its name in the header, still clickable, and "No description.".
26. Edit General's first line in the web client: the header follows, and an open Channel info updates. Give General an icon in the web client: Channel info shows it.
27. Give a channel a long name and a long first line, and narrow the window: both truncate cleanly before the search box and the buttons. DMs, groups and Saved Notes show no line and aren't clickable.

**Delete a channel**
28. Right-click "orphan": Delete channel… is last before Copy channel ID, in red. Choose it: "Delete orphan?" with "Once it's deleted, there's no going back." Cancel keeps it.
29. Open "orphan" and delete it: Delete shows a spinner, the dialog closes, the channel disappears, and the sidebar's first channel opens.
30. Create a category with one channel in it and delete that channel: the category stays, as a heading with no rows, folded or not.
31. Open a channel in RuSToat and delete it from the web client: it disappears within a moment and another channel opens, with no pane or spinner left behind. With its Channel info open instead, the dialog closes.

**A member's view**
32. As A, create an invite to M19 Test (the header's Create invite). Switch RuSToat to C and join with it ("+" → Join a server…).
33. As C: channel menus have no Create channel…, Create category… or Delete channel…; category headings offer only Copy category ID; right-clicking the empty space does nothing. Server info and Channel info work. DMs and groups never offer Delete channel….
34. As C, open a channel; switch to A and delete it; switch back to C: it's gone for C too, and C is on another channel.
35. As A, right-click M19 Test in the rail: no Leave server… (A owns it).

**Leave a server**
36. In the web client as A, set M19 Test's "user left" system message channel to General.
37. As C, right-click M19 Test: Leave server… is last before Copy server ID, in red. Choose it: "Leave M19 Test?", "You won't be able to rejoin unless you are re-invited.", and "Don't notify others that you've left", unticked. Cancel keeps the server.
38. With M19 Test on screen, leave with the box unticked: Leave shows a spinner, the server disappears from the rail, Home opens, and "Left M19 Test." shows. The web client shows C left in General.
39. Join again with a new invite and leave with the box ticked: no message in the web client. Switch back to A. Keep M19 Test: Part 3 uses it.

## Part 2: Groups and DMs (milestone 20)

**Create groups**
40. As A, on the Friends page click "New group": "Create a new group" opens with "Group name" (0/32) focused, "Select members to add", and the picker. Search narrows it by display name and by username; picks stay picked while hidden; clicking rows doesn't take the focus from the search field. Create is disabled with a blank name or one over 32 characters.
41. Name it "M20 One", pick B and C, and press Enter in the name: it opens, appears in Home, and B's web client shows it.
42. Click the "+" at the right of "Direct Messages" (tooltip "Create a group"); it doesn't move when Home's list scrolls. Create "Solo" with nobody picked: a group of one opens.
43. Right-click B on the Friends page: "Create group with…" opens the dialog with B picked ("1 selected"). Close it. Open B's card, its "…", and "Create group with…" again: B is picked. Create "M20 Two" with just B.

**Follow membership live**
44. As B in the web client, create "B's Group" with A and C (in that order): it appears in A's Home.
45. Open B's Group in RuSToat with the member list showing. As B, add a friend of B's whom A doesn't know: they appear in the member list. As B, remove them: they disappear.
46. As B, remove A from B's Group while it's open in RuSToat: it disappears from Home and Home's first conversation opens. As B, add A back: it reappears.
47. As A in the web client, leave B's Group: it disappears from RuSToat too. As B, add A back again.
48. As B, leave B's Group (B owns it): the "gave ownership to" message names the first other member in the group's order (C, since A was added back last). Switch RuSToat to C: right-click A in B's Group's member list, and "Remove from group…" and "Make group owner…" are offered. As C, make A the owner, and switch back to A.

**Add, remove and invite**
49. Open M20 Two: the header shows "Add friends to group" (person with a plus) and Create invite (a link). The picker leaves out B. Add C and another friend: both appear in the member list at once, and the group shows the "added" messages.
50. Make one add fail (for example, have a friend unfriend A while the dialog is open): the error names them, anyone added before stays added and leaves the list, the rest stay picked.
51. In M20 Two, right-click C in the member list and on one of C's messages: "Remove from group…" asks "Remove C?" with "Are you sure you want to remove C from M20 Two?". While Stoat answers, both buttons are disabled and Escape does nothing. Remove: C disappears, and B's web client shows the removal. It isn't offered on A, in a group A doesn't own, or for C in another channel.
52. Create an invite for M20 Two from its header, and again from its right-click menu: the title is "Invite people to M20 Two" with no "To #…" line. Switch RuSToat to C and join with the link: M20 Two appears for C. Switch back to A.
53. A DM and Saved Notes offer neither header button nor Create invite….

**Leave and hand over**
54. As A, right-click B in M20 Two's member list (and on one of B's messages): "Make group owner…" asks "Make B the owner?" with "You'll stay in the group, but only B will be able to remove members or hand ownership on." Make owner: the "gave ownership" message shows, and A is no longer offered "Remove from group…" or "Make group owner…". Neither shows for a plain member or on yourself.
55. As A (now a plain member), right-click M20 Two in Home: "Leave group…" (red) asks "Leave M20 Two?" with "You won't be able to rejoin unless you are re-invited." and "Don't notify others that you've left". Tick it and Leave: it leaves Home, the toast says "Left M20 Two.", and B's web client shows no "left" message.
56. Leave B's Group (A owns it now, with C): "New owner" shows C. Untick the box and Leave: C becomes owner, and the web client shows the "left" message.
57. Open "Leave group…" on M20 One (A owns it, with B and C): "New owner" shows B (the first other member). Pick C and leave: C becomes owner (the "gave ownership" message names C).
58. Open "Leave group…" on Solo: "You're the only member, so leaving deletes the group.", no checkbox, and the button says "Delete group". Delete it: it's gone.

**Close a DM**
59. Right-click A's DM with B in Home: "Close conversation…" (red) asks "Close conversation with B?" with "You can re-open it later, but it will disappear on both sides." Cancel and Escape keep it. Groups and Saved Notes don't offer it.
60. Open the DM and close it: it leaves Home here and in B's web client, and Home's first conversation opens. While Stoat answers, both buttons are disabled and Escape does nothing.
61. Open B's card and Message: the DM comes back with its history. Close it again and use the Friends page's Message: it comes back again.

**Mutuals**
62. Create a group with B and C again, and make sure A and B share a server. Open B's card: once it has loaded, "MUTUAL FRIENDS" (C) and "MUTUAL SERVERS AND GROUPS" (the server, then the group) show after Roles; hovering an icon names it. With more than 8 in a row, 8 show and then "+N".
63. Hover a row: it highlights. Click it: the card closes and the list opens, titled "Mutual friends" or "Mutual servers and groups", scrolling if long. Click C: the list closes and C's card opens where the mouse was. In the other list, click the server: it opens; click the group: it opens in Home.
64. A's own card, a bot's card and a blocked user's card show neither row.
65. Open a card near the bottom of the window: when the rows arrive, the card grows and stays inside it. Leave a shared server and reopen the card: the server is gone from the row.

## Part 3: Server list and accounts (milestone 21)

**Server order**
66. Reorder servers in the web client: RuSToat's rail follows within a moment.
67. Quit and restart RuSToat: the rail opens in the web client's order, with no jump once the fetch lands.
68. Make a folder of two servers in the web client: RuSToat shows it in the folder's place, holding those two servers.

**Folders from menus**
69. Right-click a server → Add to folder → New folder: a "New Folder" appears in its place. Add a second server to it from that server's menu. Click the folder to close it: the preview shows both icons (a server without an icon shows its initial).
70. Right-click the folder → Edit folder…: change its name and colour and Save: the tooltip and tint change, and the web client shows them.
71. Close and open the folder in the web client: RuSToat follows. Close it in RuSToat: the web client follows.
72. With the folder closed, have B make a server inside unread: the folder's pill lights. Have B mention A there: the folder shows the summed badge. Open a server inside: the folder shows the tall marker.
73. ⌘⌥↑/↓ passes over the servers in the closed folder, and walks those in an open one.
74. Remove from folder on one of its servers: it lands just below the folder. Make the folder again with two servers, then Delete folder: its servers stay in place, with no confirmation.

**Dragging in the rail**
75. Drag a server between two others (top or bottom quarter of a neighbour): a line shows where, the dragged icon fades and its icon follows the pointer; drop: it moves there.
76. Drag a server onto the middle of another: a ring shows; drop: a "New Folder" appears in the target's place, holding the target then the dragged server.
77. Drag a server onto a closed folder's middle: it goes to the end of the folder. Onto an open folder's icon: the same; just below the icon: it goes first in the folder.
78. With a folder open, drag one of its servers above or below another in it: it reorders within the folder. Drag it to the top or bottom quarter of a top-level server: it leaves the folder (the folder goes if it was the last).
79. Drag a folder (open and closed) above and below servers and other folders: it moves whole; over a server's middle there's no ring.
80. Drag over Home: the line shows above the first item; over "+" or the space below it: below the last.
81. Drop where the item already is, or onto itself: nothing changes, no "Couldn't save the server list" toast, and the web client doesn't change.
82. Press Esc mid-drag: the drag ends, nothing moves, and the open channel doesn't jump to the present. Drag off the rail and let go: nothing moves.
83. After each drop above, the web client shows the same order and folders.
84. Clicking servers and folders still opens them and toggles folders (including a click with a pixel of movement); right-click menus still open, and the "+" menu still opens.
85. With enough servers that the rail scrolls (shrink the window), drags near its ends still drop where shown (no auto-scroll).
86. Arrange folders in RuSToat (names, colours, one closed), then in the web client, then in RuSToat again: each follows the other, and nothing is lost (names, colours, collapsed state).

**Dragging accounts**
87. With A, C and D signed in, drag the third onto the top half of the first: a line shows above it, its avatar follows the pointer and its slot fades. Drop: it's first. ⌘1 opens it, and the Accounts menu lists it first.
88. Drag an account onto the bottom half of the last: it goes last. Drop one on itself, or just below the account above it: nothing changes.
89. Press Esc mid-drag: the drag ends, nothing moves, and the open channel doesn't jump to the present. Drag an account off the switcher (onto the rail or the chat) and let go: nothing moves, and the rail shows no line.
90. Clicking an account still switches to it (including a click with a pixel of movement); right-click still offers Log Out.
91. Quit and restart: the account order is kept, and the rail still opens in the web client's order.

## Part 4: Across all three

**Two accounts**
92. With A and C both signed in, switch between them and check each acts for the account on screen: the "+" menu, Stoat → Create Server…, server and channel menus (create, delete, leave), New group, the Home "+", Create group with…, Add friends to group, Remove, Make owner, Leave, Close conversation, Create invite, the mutuals lists, and folder edits and drags (each account's rail is its own).

**Light and dark themes**
93. Milestone 19: every dialog (Create a server, Create channel, Create a new category, Rename category, Delete channel, Delete category, Leave, Server info, Channel info), the counters, the Acceptable Use Policy link, "in {CATEGORY}", the radios, the checkbox, the red menu items, "No description." and the header's description line are readable.
94. Milestone 20: the friend picker's rows, checkboxes and hover, Create a new group, Add friends to group, the Remove, Make owner, Leave (with its New owner dropdown and checkbox) and Close conversation dialogs, the card's mutual rows and their hover, and the mutuals list are readable.
95. Milestone 21: folder tints (closed and open), the 2×2 preview, the Edit folder dialog's palette and ring, the rail's drop line and ring, and the faded dragged items in the rail and the switcher are visible.

**Clean up**
96. Delete M19 Test from the web client (Server settings): it leaves RuSToat's rail, and out of its folder if it was in one. (RuSToat gets Delete server in milestone 23.) Leave or delete the test groups.
