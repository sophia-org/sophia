{
  description = "Sophia: pinned Nix build of the desktop session binaries (niltempus n002)";

  inputs = {
    # The reviewed nixpkgs revision shared by the niltempus product flakes.
    nixpkgs.url = "github:NixOS/nixpkgs/c59305bab2065cfecc4944690d9eedbb56f3a9fa";
    crane.url = "github:ipetkov/crane";
    # Supplies the exact toolchain rust-toolchain.toml names; nixpkgs has a newer one.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, crane, rust-overlay }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      lib = pkgs.lib;
      toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

      # The tracked tree without the flake files, so editing them does not
      # rebuild Sophia.
      src = lib.cleanSourceWith {
        src = lib.cleanSource ./.;
        filter = path: _: !(builtins.elem (baseNameOf path) [ "flake.nix" "flake.lock" ]);
      };

      common = {
        inherit src;
        pname = "sophia";
        strictDeps = true;
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [
          pkgs.seatd
          pkgs.systemdMinimal # libudev
          pkgs.libinput
          pkgs.libgbm
          pkgs.libxkbcommon
          pkgs.linux-pam
        ];
        # The packages and features the niltempus desktop release builds.
        cargoExtraArgs = lib.concatStringsSep " " [
          "--locked"
          "-p sophia-cli --features sophia-cli/native-session"
          "-p sophia-factotum"
          "-p sophia-factotum-pam"
        ];
      };
      cargoArtifacts = craneLib.buildDepsOnly common;

      # Sophia's GPU stack, scoped to Sophia alone. Nixpkgs' libgbm and
      # libglvnd look for Mesa under /run/opengl-driver, and libglvnd also
      # under /usr/share, whose vendor files name the host's Mesa. Neither
      # suits a non-NixOS host. These copies look only at the pinned Mesa,
      # with no environment variables and no global state, so programs that
      # Sophia starts keep the host's own graphics stack.
      mesa = pkgs.mesa;
      gbmBackendsFlag = "-Dgbm-backends-path=${pkgs.addDriverRunpath.driverLink}/lib/gbm";
      sophiaGbm = pkgs.libgbm.overrideAttrs (old: {
        # Built from the pinned Mesa's source, matching its GBM backend.
        inherit (mesa) version src;
        mesonFlags =
          assert lib.assertMsg (builtins.elem gbmBackendsFlag old.mesonFlags)
            "libgbm no longer sets ${gbmBackendsFlag}";
          map (flag: if flag == gbmBackendsFlag then "-Dgbm-backends-path=${mesa}/lib/gbm" else flag)
            old.mesonFlags;
      });
      eglVendorDirs = "${pkgs.addDriverRunpath.driverLink}/share/glvnd/egl_vendor.d:/etc/glvnd/egl_vendor.d:/usr/share/glvnd/egl_vendor.d";
      sophiaGlvnd = pkgs.libglvnd.overrideAttrs (old: {
        env = old.env // {
          NIX_CFLAGS_COMPILE =
            assert lib.assertMsg (lib.hasInfix eglVendorDirs old.env.NIX_CFLAGS_COMPILE)
              "libglvnd no longer sets its EGL vendor directories as expected";
            builtins.replaceStrings [ eglVendorDirs ] [ "${mesa}/share/glvnd/egl_vendor.d" ]
              old.env.NIX_CFLAGS_COMPILE;
        };
      });

      sophia = craneLib.buildPackage (common // {
        inherit cargoArtifacts;
        doCheck = false;
        # The PAM helper must use the host's PAM: the host's pam_unix runs its
        # setuid unix_chkpwd, which Nix's PAM cannot. Drop Nix's linux-pam from
        # the search path and look in /usr/lib after the Nix libraries. DT_RPATH,
        # unlike DT_RUNPATH, also covers libpam's own dependencies. The host
        # libpam needs only libc (GLIBC_2.34 or older), which the Nix glibc
        # already loaded provides. This is the one deliberate host dependency.
        #
        # sophia-xshmfence loads libxshmfence for DRI3 X clients; give it the
        # pinned one.
        postFixup = ''
          # The scoped GPU stack comes first, ahead of the libgbm Sophia
          # links (same soname and ABI); libEGL.so.1 is loaded at run time.
          patchelf --set-rpath "${sophiaGbm}/lib:${sophiaGlvnd}/lib:$(patchelf --print-rpath $out/bin/sophia)" $out/bin/sophia
          patchelf --add-rpath ${pkgs.libxshmfence}/lib $out/bin/sophia
          helper=$out/bin/sophia-factotum-pam
          kept=$(patchelf --print-rpath "$helper" | tr ':' '\n' | grep -v '${pkgs.linux-pam}' | paste -sd: -)
          patchelf --force-rpath --set-rpath "''${kept:+$kept:}/usr/lib" "$helper"
        '';
      });

      # Every workspace crate and feature, as the gate builds them.
      workspaceCommon = common // {
        pname = "sophia-workspace";
        cargoExtraArgs = "--locked";
        nativeBuildInputs = common.nativeBuildInputs ++ [ pkgs.python3 pkgs.git ];
        buildInputs = common.buildInputs ++ [ pkgs.libdrm ];
      };
      workspaceArgs = workspaceCommon // {
        cargoArtifacts = craneLib.buildDepsOnly (workspaceCommon // {
          cargoCheckExtraArgs = "--workspace --all-features --all-targets";
          cargoTestExtraArgs = "--workspace --all-features --no-run";
        });
      };
    in
    {
      packages.${system} = {
        inherit sophia;
        default = sophia;
        # Sophia's scoped GPU stack, exported for inspection and host checks.
        sophia-gbm = sophiaGbm;
        sophia-glvnd = sophiaGlvnd;
        # Not yet a check: the gate's workspace tests assume a host with
        # /usr/bin/bwrap, /usr/bin/true, sleep, sh, xterm, Go and a C compiler,
        # and protection domains that see only /usr. Under Nix's build sandbox
        # 6610 pass and 107 fail (development-evidence/n002-sophia-nix-01).
        # The isolated host gate stays authoritative until the sandboxes can
        # see /nix/store (phase B) and the tests run in an FHS test root.
        workspace-tests = craneLib.cargoTest (workspaceArgs // {
          cargoTestExtraArgs = "--workspace --all-features --no-fail-fast";
          preBuild = ''
            export HOME=$TMPDIR XDG_CONFIG_HOME=$TMPDIR/test-config
            mkdir -m 0700 "$XDG_CONFIG_HOME"
          '';
        });
      };

      checks.${system} = {
        inherit sophia;
        format = craneLib.cargoFmt { inherit src; pname = "sophia"; };
        # The workspace gate's lint step (crates/xtask/src/check.rs).
        clippy = craneLib.cargoClippy (workspaceArgs // {
          CLIPPY_CONF_DIR = "${src}";
          cargoClippyExtraArgs = "--workspace --all-features --all-targets -- -D warnings";
        });
      };

      # Tools and environment only: no filesystem, device or network isolation.
      # The isolated wrappers stay the gate.
      devShells.${system}.default = craneLib.devShell {
        packages = [ pkgs.pkg-config ] ++ common.buildInputs;
      };
    };
}
