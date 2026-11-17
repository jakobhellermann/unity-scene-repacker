#:package Mono.Cecil@0.11.6
using Mono.Cecil;

// usage: retarget.cs <input.dll> <output.dll> <directory of its dependencies>
// netstandard2.0 projects fail to compile against assemblies referencing netstandard 2.1 (CS1705).
var resolver = new DefaultAssemblyResolver();
resolver.AddSearchDirectory(args[2]);
using var assembly = AssemblyDefinition.ReadAssembly(args[0], new ReaderParameters { ReadingMode = ReadingMode.Immediate, AssemblyResolver = resolver });

var netstandard = assembly.MainModule.AssemblyReferences.Single(reference => reference.Name == "netstandard");
if (netstandard.Version != new Version(2, 1, 0, 0)) throw new Exception($"{args[0]} references netstandard {netstandard.Version}, expected 2.1");
netstandard.Version = new Version(2, 0, 0, 0);

assembly.Write(args[1]);
