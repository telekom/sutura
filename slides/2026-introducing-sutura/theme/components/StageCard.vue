<script setup lang="ts">
defineProps<{
  label: string
  name: string
}>()
</script>

<template>
  <div class="stage-card">
    <div class="mono-label text-teal-300" style="font-size:1rem">{{ label }}</div>
    <div class="j-serif mt-2 text-3xl leading-snug text-white">{{ name }}</div>
    <slot />
  </div>
</template>
