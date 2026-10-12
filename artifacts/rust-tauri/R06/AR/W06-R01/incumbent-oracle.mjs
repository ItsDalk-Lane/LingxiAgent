import YAML from '/Users/study_superior/.codex/worktrees/r06-independent-review/LingxiAgent/node_modules/js-yaml/index.js';
const rows=[
 ['nonstring-root-key','? [memory]\n: {enabled: false}\n'],
 ['nonstring-sensitive-key','memory:\n  ? [enabled]\n  : false\n'],
 ['complex-agent-key','? [agent]\n: {name: "遗失姓名"}\nmemory: {enabled: false}\n'],
 ['complex-name-key','agent:\n  ? [name]\n  : "遗失姓名"\nmemory: {enabled: false}\n'],
 ['complex-discovery-key','memory: {enabled: false}\nworkspace_context:\n  ? [discover_project_skills]\n  : false\n'],
 ['complex-skills-peer','skills:\n  ? [enabled]\n  : [shared-review]\n'],
 ['canonical-duplicate','memory: {enabled: false}\n? [memory]\n: {enabled: true}\n'],
];
for(const [name,yaml] of rows){try{console.log(JSON.stringify({name,yaml,incumbent:YAML.load(yaml)}));}catch(e){console.log(JSON.stringify({name,yaml,error:e.message}));}}
