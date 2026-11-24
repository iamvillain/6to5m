//! RDR2 meta class and enum definition tables (RSC8 meta files: ytyp, ymap).
//! GENERATED from the MLO format spec's defs.json (tables byte-identical in
//! every vanilla, CodeX-written and working RedM file that carries them);
//! do not edit by hand. Field names are joaat hashes; labels are comments.

use super::meta::{ClassDef, EnumDef};

/// 0xA11E83E5 (0xA11E83E5), 80 bytes, 1 carrier files
pub const C_A11E83E5: ClassDef = ClassDef {
    name: 0xA11E83E5, key: 2939644351, unk8: 1024, size: 80,
    entries: &[
        (0xFE30E6F3, 16, 0x33, 0, 0, 0x00000000), // min
        (0x606068C4, 32, 0x33, 0, 0, 0x00000000), // max
        (0x56892E3F, 48, 0x33, 0, 0, 0x00000000), // 0x56892E3F
        (0xEFDCADA0, 64, 0x62, 0, 0, 0x9F874F86), // 0xEFDCADA0
    ],
};

/// 0xA15F529D (0xA15F529D), 80 bytes, 2 carrier files
pub const C_A15F529D: ClassDef = ClassDef {
    name: 0xA15F529D, key: 545504399, unk8: 1024, size: 80,
    entries: &[
        (0x01166164, 16, 0x33, 0, 0, 0x00000000), // position
        (0x96BEECA0, 32, 0x21, 0, 0, 0x00000000), // width
        (0xBCA972D6, 36, 0x21, 0, 0, 0x00000000), // depth
        (0x52FDF336, 40, 0x21, 0, 0, 0x00000000), // height
        (0x32F58784, 48, 0x33, 0, 0, 0x00000000), // forward
        (0xB8CCC339, 64, 0x33, 0, 0, 0x00000000), // right
    ],
};

/// 0xE5B7A9F2 (0xE5B7A9F2), 48 bytes, 1 carrier files
pub const C_E5B7A9F2: ClassDef = ClassDef {
    name: 0xE5B7A9F2, key: 692839202, unk8: 1024, size: 48,
    entries: &[
        (0xE6E17F14, 8, 0x4A, 0, 0, 0x00000000), // name
        (0xD8950703, 16, 0x33, 0, 0, 0x00000000), // offsetPosition
        (0x00000100, 0, 0x05, 0, 0, 0xA11E83E5), // (array element)
        (0x4DC20166, 32, 0x52, 0, 2, 0x00000000), // 0x4DC20166
    ],
};

/// 0xFB36133A (0xFB36133A), 48 bytes, 8 carrier files
pub const C_FB36133A: ClassDef = ClassDef {
    name: 0xFB36133A, key: 4066526927, unk8: 1024, size: 48,
    entries: &[
        (0x8AA47699, 16, 0x05, 0, 0, 0x3F4F4469), // bound rage__spdSphere
        (0x67D26872, 32, 0x65, 0, 32, 0x00000000), // flags
    ],
};

/// CBaseArchetypeDef (0x82D6FC83), 160 bytes, 8 carrier files
pub const CBASEARCHETYPEDEF: ClassDef = ClassDef {
    name: 0x82D6FC83, key: 1841055807, unk8: 1024, size: 160,
    entries: &[
        (0xC067A638, 8, 0x17, 0, 0, 0x00000000), // loadFlags
        (0xBF74DFA7, 16, 0x21, 0, 0, 0x00000000), // lodDist
        (0x67D26872, 20, 0x15, 0, 0, 0x00000000), // flags
        (0x8F215E15, 24, 0x15, 0, 0, 0x00000000), // avoidanceflags
        (0x6C1523E4, 28, 0x15, 0, 0, 0x00000000), // specialAttribute
        (0xD9EF8236, 32, 0x33, 0, 0, 0x00000000), // bbMin
        (0xE78AA618, 48, 0x33, 0, 0, 0x00000000), // bbMax
        (0xD61872FF, 64, 0x33, 0, 0, 0x00000000), // bsCentre
        (0x82E8C5DE, 80, 0x21, 0, 0, 0x00000000), // bsRadius
        (0xAD5D5B4C, 84, 0x21, 0, 0, 0x00000000), // hdTextureDist
        (0xE6E17F14, 88, 0x4A, 0, 0, 0x00000000), // name
        (0x75D215A1, 92, 0x4A, 0, 0, 0x00000000), // textureDictionary
        (0x19471791, 96, 0x4A, 0, 0, 0x00000000), // clipDictionary
        (0xFC1FB6FB, 100, 0x4A, 0, 0, 0x00000000), // drawableDictionary
        (0xD3C717FC, 104, 0x4A, 0, 0, 0x00000000), // physicsDictionary
        (0x069004B2, 108, 0x62, 0, 0, 0x76BAF7C7), // assetType rage__fwArchetypeDef__eAssetType
        (0x9A77C7C5, 112, 0x4A, 0, 0, 0x00000000), // assetName
        (0x00000100, 0, 0x07, 0, 0, 0x00000000), // (array element)
        (0xF17E7F28, 120, 0x52, 0, 17, 0x00000000), // extensions
        (0x9A7B6E6D, 136, 0x17, 0, 0, 0x00000000), // guid
        (0x07D164A8, 144, 0x62, 0, 0, 0x079525DA), // 0x07D164A8 MapEntityType
    ],
};

/// CBlockDesc (0xB7206E4A), 72 bytes, 14 carrier files
pub const CBLOCKDESC: ClassDef = ClassDef {
    name: 0xB7206E4A, key: 794832344, unk8: 768, size: 72,
    entries: &[
        (0x68C27E33, 0, 0x15, 0, 0, 0x00000000), // version
        (0x67D26872, 4, 0x15, 0, 0, 0x00000000), // flags
        (0xE6E17F14, 8, 0x44, 0, 0, 0x00000000), // name
        (0x76350055, 24, 0x44, 0, 0, 0x00000000), // exportedBy
        (0x8C060170, 40, 0x44, 0, 0, 0x00000000), // owner
        (0x0F678E23, 56, 0x44, 0, 0, 0x00000000), // time
    ],
};

/// CDistantLODLight (0x793441F4), 96 bytes, 14 carrier files
pub const CDISTANTLODLIGHT: ClassDef = ClassDef {
    name: 0x793441F4, key: 1297336104, unk8: 768, size: 96,
    entries: &[
        (0x00000100, 0, 0x05, 0, 0, 0xE2CBCFD4), // (array element) FloatXYZ
        (0x01166164, 8, 0x52, 0, 0, 0x00000000), // position
        (0x00000100, 0, 0x15, 0, 0, 0x00000000), // (array element)
        (0x94720DEC, 24, 0x52, 0, 2, 0x00000000), // RGB
        (0x00000100, 0, 0x21, 0, 0, 0x00000000), // (array element)
        (0xEFCD993D, 40, 0x52, 0, 4, 0x00000000), // intensity
        (0xDD11304B, 56, 0x13, 0, 0, 0x00000000), // numStreetLights
        (0x7A5C560D, 58, 0x13, 0, 0, 0x00000000), // category
        (0x00000100, 0, 0x15, 0, 0, 0x00000000), // (array element)
        (0xFFEC781A, 64, 0x52, 0, 8, 0x00000000), // powerGridId
        (0x00000100, 0, 0x15, 0, 0, 0x00000000), // (array element)
        (0x3E818018, 80, 0x52, 0, 10, 0x00000000), // hash
    ],
};

/// CEntityDef (0xCE501483), 144 bytes, 16 carrier files
pub const CENTITYDEF: ClassDef = ClassDef {
    name: 0xCE501483, key: 612398007, unk8: 1024, size: 144,
    entries: &[
        (0xA023A02C, 8, 0x4A, 0, 0, 0x00000000), // archetypeName
        (0x67D26872, 12, 0x15, 0, 0, 0x00000000), // flags
        (0x1B60404D, 16, 0x08, 0, 0, 0x00000000), // id
        (0x01166164, 32, 0x33, 0, 0, 0x00000000), // position
        (0x77DD4010, 48, 0x34, 0, 0, 0x00000000), // rotation
        (0x9CA32637, 64, 0x21, 0, 0, 0x00000000), // scaleXY
        (0x10FB7C42, 68, 0x21, 0, 0, 0x00000000), // scaleZ
        (0xD89231BD, 72, 0x14, 0, 0, 0x00000000), // parentIndex
        (0xBF74DFA7, 76, 0x21, 0, 0, 0x00000000), // lodDist
        (0xCA974BCD, 80, 0x21, 0, 0, 0x00000000), // childLodDist
        (0x6C8F1715, 84, 0x62, 0, 0, 0x4B5ACC2F), // lodLevel rage__eLodType
        (0xA687AC89, 88, 0x15, 0, 0, 0x00000000), // numChildren
        (0x2691F019, 92, 0x62, 0, 0, 0x26A60207), // priorityLevel rage__ePriorityLevel
        (0x00000100, 0, 0x07, 0, 0, 0x00000000), // (array element)
        (0xF17E7F28, 96, 0x52, 0, 13, 0x00000000), // extensions
        (0xFFEC781A, 112, 0x15, 0, 0, 0x00000000), // powerGridId
        (0x3C852527, 128, 0x15, 0, 0, 0x00000000), // tintValue
        (0x0DC5F270, 132, 0x11, 0, 0, 0x00000000), // blendAgeLayer
        (0x540CA436, 133, 0x11, 0, 0, 0x00000000), // blendAgeDirt
    ],
};

/// CExtensionDefAudioCollisionSettings (0x15DEDA27), 48 bytes, 1 carrier files
pub const CEXTENSIONDEFAUDIOCOLLISIONSETTINGS: ClassDef = ClassDef {
    name: 0x15DEDA27, key: 3591012747, unk8: 1024, size: 48,
    entries: &[
        (0xE6E17F14, 8, 0x4A, 0, 0, 0x00000000), // name
        (0xD8950703, 16, 0x33, 0, 0, 0x00000000), // offsetPosition
        (0xA4FEB6F7, 32, 0x4A, 0, 0, 0x00000000), // settings
    ],
};

/// CExtensionDefDoor (0x752DC011), 96 bytes, 2 carrier files
pub const CEXTENSIONDEFDOOR: ClassDef = ClassDef {
    name: 0x752DC011, key: 2215730182, unk8: 1024, size: 96,
    entries: &[
        (0xE6E17F14, 8, 0x4A, 0, 0, 0x00000000), // name
        (0xD8950703, 16, 0x33, 0, 0, 0x00000000), // offsetPosition
        (0xDAF8214E, 32, 0x01, 0, 0, 0x00000000), // autoOpens
        (0x75F9B59A, 33, 0x01, 0, 0, 0x00000000), // enableLimitAngle
        (0x091A42D0, 34, 0x01, 0, 0, 0x00000000), // 0x091A42D0
        (0xBF01E4BB, 35, 0x01, 0, 0, 0x00000000), // startsLocked
        (0xA45138A8, 36, 0x01, 0, 0, 0x00000000), // canBreak
        (0xA37CAB71, 37, 0x01, 0, 0, 0x00000000), // 0xA37CAB71
        (0xCFE37BDB, 40, 0x21, 0, 0, 0x00000000), // 0xCFE37BDB
        (0xA0CF3C8D, 44, 0x21, 0, 0, 0x00000000), // 0xA0CF3C8D
        (0x9C555ECF, 48, 0x21, 0, 0, 0x00000000), // 0x9C555ECF
        (0xBCA0289E, 52, 0x21, 0, 0, 0x00000000), // 0xBCA0289E
        (0x2DEBE103, 56, 0x21, 0, 0, 0x00000000), // doorTargetRatio
        (0x0D5B0930, 60, 0x4A, 0, 0, 0x00000000), // audioHash
        (0x00000100, 0, 0x4A, 0, 0, 0x00000000), // (array element)
        (0x5907D538, 64, 0x53, 0, 14, 0x00000004), // doorTags
    ],
};

/// CExtensionDefLadder (0x821D5421), 96 bytes, 1 carrier files
pub const CEXTENSIONDEFLADDER: ClassDef = ClassDef {
    name: 0x821D5421, key: 858599873, unk8: 1024, size: 96,
    entries: &[
        (0xE6E17F14, 8, 0x4A, 0, 0, 0x00000000), // name
        (0xD8950703, 16, 0x33, 0, 0, 0x00000000), // offsetPosition
        (0xEEE0A2B6, 32, 0x33, 0, 0, 0x00000000), // bottom
        (0x4C5DD09A, 48, 0x33, 0, 0, 0x00000000), // top
        (0x4F485502, 64, 0x33, 0, 0, 0x00000000), // normal
        (0x3798B2FE, 80, 0x62, 0, 0, 0x4D24FF09), // materialType CExtensionDefLadderMaterialType
        (0xBEE46552, 84, 0x4A, 0, 0, 0x00000000), // template
        (0x21AAC4F9, 88, 0x01, 0, 0, 0x00000000), // canGetOffAtTop
        (0x370EFEA8, 89, 0x01, 0, 0, 0x00000000), // canGetOffAtBottom
    ],
};

/// CExtensionDefLightEffect (0x27922C43), 64 bytes, 7 carrier files
pub const CEXTENSIONDEFLIGHTEFFECT: ClassDef = ClassDef {
    name: 0x27922C43, key: 3957697751, unk8: 1024, size: 64,
    entries: &[
        (0xE6E17F14, 8, 0x4A, 0, 0, 0x00000000), // name
        (0xD8950703, 16, 0x33, 0, 0, 0x00000000), // offsetPosition
        (0x00000100, 0, 0x05, 0, 0, 0xF54B227B), // (array element) CLightAttrDef
        (0x10579DF2, 32, 0x52, 0, 2, 0x00000000), // instances
        (0x00000100, 0, 0x05, 0, 0, 0xF54B227B), // (array element) CLightAttrDef
        (0xC4E96A00, 48, 0x52, 0, 4, 0x00000000), // additionalInstances
    ],
};

/// CExtensionDefStairs (0x0C55EA5F), 112 bytes, 2 carrier files
pub const CEXTENSIONDEFSTAIRS: ClassDef = ClassDef {
    name: 0x0C55EA5F, key: 954921626, unk8: 1024, size: 112,
    entries: &[
        (0xE6E17F14, 8, 0x4A, 0, 0, 0x00000000), // name
        (0xD8950703, 16, 0x33, 0, 0, 0x00000000), // offsetPosition
        (0xEEE0A2B6, 32, 0x33, 0, 0, 0x00000000), // bottom
        (0x4C5DD09A, 48, 0x33, 0, 0, 0x00000000), // top
        (0x6A641A32, 64, 0x33, 0, 0, 0x00000000), // boundMin
        (0xFCE94826, 80, 0x33, 0, 0, 0x00000000), // boundMax
        (0x00000100, 0, 0x05, 0, 0, 0xA15F529D), // (array element)
        (0x81E6DC20, 96, 0x52, 0, 6, 0x00000000), // steps
    ],
};

/// CLODLight (0x87AC4538), 152 bytes, 14 carrier files
pub const CLODLIGHT: ClassDef = ClassDef {
    name: 0x87AC4538, key: 1438186190, unk8: 768, size: 152,
    entries: &[
        (0x00000100, 0, 0x05, 0, 0, 0xE2CBCFD4), // (array element) FloatXYZ
        (0x34DF9828, 8, 0x52, 0, 0, 0x00000000), // direction
        (0x00000100, 0, 0x21, 0, 0, 0x00000000), // (array element)
        (0xCC6806E8, 24, 0x52, 0, 2, 0x00000000), // falloff
        (0x00000100, 0, 0x21, 0, 0, 0x00000000), // (array element)
        (0x2BB342BA, 40, 0x52, 0, 4, 0x00000000), // falloffExponent
        (0x00000100, 0, 0x15, 0, 0, 0x00000000), // (array element)
        (0xB983BBE6, 56, 0x52, 0, 6, 0x00000000), // timeAndStateFlags
        (0x00000100, 0, 0x15, 0, 0, 0x00000000), // (array element)
        (0x3E818018, 72, 0x52, 0, 8, 0x00000000), // hash
        (0x00000100, 0, 0x11, 0, 0, 0x00000000), // (array element)
        (0x455C3938, 88, 0x52, 0, 10, 0x00000000), // coneInnerAngle
        (0x00000100, 0, 0x11, 0, 0, 0x00000000), // (array element)
        (0xBC76ACC0, 104, 0x52, 0, 12, 0x00000000), // coneOuterAngleOrCapExt
        (0x00000100, 0, 0x11, 0, 0, 0x00000000), // (array element)
        (0x88A2B1FB, 120, 0x52, 0, 14, 0x00000000), // coronaIntensity
        (0x00000100, 0, 0x15, 0, 0, 0x00000000), // (array element)
        (0xFFD167D6, 136, 0x52, 0, 16, 0x00000000), // 0xFFD167D6
    ],
};

/// CLightAttrDef (0xF54B227B), 352 bytes, 7 carrier files
pub const CLIGHTATTRDEF: ClassDef = ClassDef {
    name: 0xF54B227B, key: 270638961, unk8: 1024, size: 352,
    entries: &[
        (0x00000100, 0, 0x21, 0, 0, 0x00000000), // (array element)
        (0x39A625E4, 8, 0x50, 0, 0, 0x00000003), // posn
        (0x00000100, 0, 0x11, 0, 0, 0x00000000), // (array element)
        (0x5BDC13E3, 20, 0x50, 0, 2, 0x00000003), // colour
        (0xE4447B12, 23, 0x11, 0, 0, 0x00000000), // flashiness
        (0xEFCD993D, 24, 0x21, 0, 0, 0x00000000), // intensity
        (0x67D26872, 28, 0x15, 0, 0, 0x00000000), // flags
        (0x29D7F7B0, 32, 0x12, 0, 0, 0x00000000), // boneTag
        (0x1CBBBE30, 34, 0x11, 0, 0, 0x00000000), // lightType
        (0x951BDD14, 35, 0x11, 0, 0, 0x00000000), // groupId
        (0x8609D52C, 36, 0x15, 0, 0, 0x00000000), // timeFlags
        (0xCC6806E8, 40, 0x21, 0, 0, 0x00000000), // falloff
        (0x2BB342BA, 44, 0x21, 0, 0, 0x00000000), // falloffExponent
        (0x00000100, 0, 0x21, 0, 0, 0x00000000), // (array element)
        (0x64B51E10, 48, 0x50, 0, 13, 0x00000004), // cullingPlane
        (0x00000100, 0, 0x21, 0, 0, 0x00000000), // (array element)
        (0xF48DB3A6, 64, 0x50, 0, 15, 0x00000004), // cullingPlane1
        (0xDAE93379, 80, 0x11, 0, 0, 0x00000000), // shadowBlur
        (0xED7C0C96, 81, 0x11, 0, 0, 0x00000000), // shadowOpacity
        (0x8BD6DABF, 82, 0x12, 0, 0, 0x00000000), // padding2
        (0x4BAA50C5, 84, 0x21, 0, 0, 0x00000000), // roughnessMod
        (0x291D3720, 88, 0x21, 0, 0, 0x00000000), // volIntensity
        (0x78F8388F, 92, 0x21, 0, 0, 0x00000000), // volSizeScale
        (0x00000100, 0, 0x11, 0, 0, 0x00000000), // (array element)
        (0x8822FBCE, 96, 0x50, 0, 23, 0x00000003), // volOuterColour
        (0x26542706, 99, 0x11, 0, 0, 0x00000000), // lightHash
        (0xB34D77F7, 100, 0x21, 0, 0, 0x00000000), // volOuterIntensity
        (0x6ACDA1EF, 104, 0x01, 0, 0, 0x00000000), // injectsIntoVoxels
        (0x65A03C8B, 108, 0x21, 0, 0, 0x00000000), // coronaSize
        (0xA470B2E2, 112, 0x21, 0, 0, 0x00000000), // volOuterExponent
        (0x4DF55F03, 116, 0x11, 0, 0, 0x00000000), // lightFadeDistance
        (0x73E32C64, 117, 0x11, 0, 0, 0x00000000), // shadowFadeDistance
        (0xF7698288, 118, 0x11, 0, 0, 0x00000000), // specularFadeDistance
        (0x7B33E620, 119, 0x11, 0, 0, 0x00000000), // volumetricFadeDistance
        (0x38E6C28A, 120, 0x21, 0, 0, 0x00000000), // shadowNearClip
        (0x4855CDE5, 124, 0x20, 0, 0, 0x00000000), // 0x4855CDE5
        (0x0DA3D86E, 126, 0x20, 0, 0, 0x00000000), // 0x0DA3D86E
        (0x88A2B1FB, 128, 0x21, 0, 0, 0x00000000), // coronaIntensity
        (0x9639A173, 132, 0x21, 0, 0, 0x00000000), // coronaZBias
        (0x93C52D2C, 136, 0x4A, 0, 0, 0x00000000), // 0x93C52D2C
        (0x52A89335, 144, 0x33, 0, 0, 0x00000000), // 0x52A89335
        (0x48FB2B93, 160, 0x21, 0, 0, 0x00000000), // emitterSize
        (0x85E38C38, 164, 0x21, 0, 0, 0x00000000), // emitterIntensityMult
        (0x3DDB9FA8, 168, 0x21, 0, 0, 0x00000000), // emitterTintDesaturation
        (0x00000100, 0, 0x21, 0, 0, 0x00000000), // (array element)
        (0x34DF9828, 172, 0x50, 0, 44, 0x00000003), // direction
        (0x00000100, 0, 0x21, 0, 0, 0x00000000), // (array element)
        (0x8E6F0BA9, 184, 0x50, 0, 46, 0x00000003), // tangent
        (0x455C3938, 196, 0x21, 0, 0, 0x00000000), // coneInnerAngle
        (0xF8D9E344, 200, 0x21, 0, 0, 0x00000000), // coneOuterAngle
        (0xD0E057ED, 204, 0x21, 0, 0, 0x00000000), // squareAspectRatio
        (0x00000100, 0, 0x21, 0, 0, 0x00000000), // (array element)
        (0x2D3F79C0, 208, 0x50, 0, 51, 0x00000003), // extents
        (0x402D6D92, 220, 0x15, 0, 0, 0x00000000), // projectedTextureKey
        (0xE23B9F08, 224, 0x21, 0, 0, 0x00000000), // SSAOStrength
        (0x684C08C5, 228, 0x15, 0, 0, 0x00000000), // extraFlags
        (0x345D14AD, 232, 0x21, 0, 0, 0x00000000), // falloffBias
        (0x3F4C1BB0, 236, 0x21, 0, 0, 0x00000000), // 0x3F4C1BB0
        (0x9C23D4E9, 240, 0x21, 0, 0, 0x00000000), // 0x9C23D4E9
        (0x383857EA, 244, 0x21, 0, 0, 0x00000000), // wrapAmount
        (0x8ABD8E7E, 248, 0x21, 0, 0, 0x00000000), // nonDirectionality
        (0x5B239A55, 252, 0x21, 0, 0, 0x00000000), // scaleDirectional
        (0xA968034C, 256, 0x21, 0, 0, 0x00000000), // 0xA968034C
        (0x5148E7B3, 260, 0x11, 0, 0, 0x00000000), // 0x5148E7B3
        (0x83161CBE, 261, 0x01, 0, 0, 0x00000000), // 0x83161CBE
        (0xC11AF51B, 264, 0x21, 0, 0, 0x00000000), // 0xC11AF51B
        (0x53A339C3, 268, 0x21, 0, 0, 0x00000000), // 0x53A339C3
        (0x0F06E05A, 272, 0x21, 0, 0, 0x00000000), // 0x0F06E05A
        (0x724AE3FF, 276, 0x21, 0, 0, 0x00000000), // 0x724AE3FF
        (0x73D8B6CB, 280, 0x21, 0, 0, 0x00000000), // 0x73D8B6CB
        (0x2EBE653A, 284, 0x21, 0, 0, 0x00000000), // 0x2EBE653A
        (0x208D587D, 288, 0x21, 0, 0, 0x00000000), // 0x208D587D
        (0x2E0E4E3C, 292, 0x21, 0, 0, 0x00000000), // 0x2E0E4E3C
        (0xB4B05CF2, 296, 0x21, 0, 0, 0x00000000), // 0xB4B05CF2
        (0x0963B563, 300, 0x21, 0, 0, 0x00000000), // 0x0963B563
        (0x575E428D, 304, 0x21, 0, 0, 0x00000000), // dynamicShadowFadeDistance
        (0x66657D69, 308, 0x15, 0, 0, 0x00000000), // lightId
        (0xD08D1113, 312, 0x21, 0, 0, 0x00000000), // blendTime
        (0xA13C79EF, 316, 0x21, 0, 0, 0x00000000), // offset
        (0x30ECA388, 320, 0x33, 0, 0, 0x00000000), // 0x30ECA388
        (0xCED22A4C, 336, 0x15, 0, 0, 0x00000000), // overrideHash
    ],
};

/// CMapData (0xD3593FA6), 608 bytes, 14 carrier files
pub const CMAPDATA: ClassDef = ClassDef {
    name: 0xD3593FA6, key: 2199006343, unk8: 1024, size: 608,
    entries: &[
        (0xE6E17F14, 8, 0x4A, 0, 0, 0x00000000), // name
        (0xA99C4651, 12, 0x4A, 0, 0, 0x00000000), // parent
        (0x67D26872, 16, 0x15, 0, 0, 0x00000000), // flags
        (0x6A675035, 20, 0x15, 0, 0, 0x00000000), // contentFlags
        (0xDD22821F, 32, 0x33, 0, 0, 0x00000000), // streamingExtentsMin
        (0xA22EA335, 48, 0x33, 0, 0, 0x00000000), // streamingExtentsMax
        (0x1C75BCF1, 64, 0x33, 0, 0, 0x00000000), // entitiesExtentsMin
        (0x6D074437, 80, 0x33, 0, 0, 0x00000000), // entitiesExtentsMax
        (0x00000100, 0, 0x07, 0, 0, 0x00000000), // (array element)
        (0xCCAB9307, 96, 0x52, 0, 8, 0x00000000), // entities
        (0x00000100, 0, 0x05, 0, 0, 0x163022A5), // (array element) rage__fwContainerLodDef
        (0xAEFF8D15, 112, 0x52, 0, 10, 0x00000000), // containerLods
        (0x00000100, 0, 0x05, 0, 0, 0x3A282E1D), // (array element) BoxOccluder
        (0xED70C614, 128, 0x52, 4, 12, 0x00000000), // boxOccluders
        (0x00000100, 0, 0x05, 0, 0, 0xA36C4EAD), // (array element) OccludeModel
        (0x7F1998DD, 144, 0x52, 4, 14, 0x00000000), // occludeModels
        (0x00000100, 0, 0x4A, 0, 0, 0x00000000), // (array element)
        (0x38999564, 160, 0x52, 0, 16, 0x00000000), // physicsDictionaries
        (0x9920DC29, 176, 0x05, 0, 0, 0xF14A15BE), // instancedData rage__fwInstancedMapData
        (0x00000100, 0, 0x05, 0, 0, 0x674F9350), // (array element) CTimeCycleModifier
        (0xAF9C3BD9, 224, 0x52, 0, 19, 0x00000000), // timeCycleModifiers
        (0x00000100, 0, 0x05, 0, 0, 0x3BC4AC19), // (array element)
        (0x8471928B, 240, 0x52, 0, 21, 0x00000000), // 0x8471928B
        (0x00000100, 0, 0x05, 0, 0, 0x7BEF699E), // (array element) CReflectionProbeData
        (0x4D4A0635, 256, 0x52, 0, 23, 0x00000000), // reflectionProbeData
        (0x00000100, 0, 0x05, 0, 0, 0x6EE83BDF), // (array element) CCarGen
        (0xC200AB4C, 272, 0x52, 0, 25, 0x00000000), // carGenerators
        (0x69C2C0FA, 288, 0x05, 0, 0, 0x87AC4538), // LODLightsSOA CLODLight
        (0xB0199551, 440, 0x05, 0, 0, 0x793441F4), // DistantLODLightsSOA CDistantLODLight
        (0xE433D77D, 536, 0x05, 0, 0, 0xB7206E4A), // block CBlockDesc
    ],
};

/// CMapTypes (0xD98BB561), 96 bytes, 8 carrier files
pub const CMAPTYPES: ClassDef = ClassDef {
    name: 0xD98BB561, key: 249887037, unk8: 768, size: 96,
    entries: &[
        (0x00000100, 0, 0x07, 0, 0, 0x00000000), // (array element)
        (0xF17E7F28, 8, 0x52, 0, 0, 0x00000000), // extensions
        (0x00000100, 0, 0x07, 0, 0, 0x00000000), // (array element)
        (0x018A3B1B, 24, 0x52, 0, 2, 0x00000000), // archetypes
        (0x00000100, 0, 0x07, 0, 0, 0x00000000), // (array element)
        (0x069ADF93, 40, 0x52, 0, 4, 0x00000000), // txdRelationships
        (0xE6E17F14, 56, 0x4A, 0, 0, 0x00000000), // name
        (0x00000100, 0, 0x4A, 0, 0, 0x00000000), // (array element)
        (0x3C6F8844, 64, 0x52, 0, 7, 0x00000000), // dependencies
        (0x00000100, 0, 0x05, 0, 0, 0x46AD6DFF), // (array element) CCompositeEntityType
        (0x65E9D4EB, 80, 0x52, 0, 9, 0x00000000), // compositeEntityTypes
    ],
};

/// CMloArchetypeDef (0x10506455), 352 bytes, 8 carrier files
pub const CMLOARCHETYPEDEF: ClassDef = ClassDef {
    name: 0x10506455, key: 3126831897, unk8: 1024, size: 352,
    entries: &[
        (0xC067A638, 8, 0x17, 0, 0, 0x00000000), // loadFlags
        (0xBF74DFA7, 16, 0x21, 0, 0, 0x00000000), // lodDist
        (0x67D26872, 20, 0x15, 0, 0, 0x00000000), // flags
        (0x8F215E15, 24, 0x15, 0, 0, 0x00000000), // avoidanceflags
        (0x6C1523E4, 28, 0x15, 0, 0, 0x00000000), // specialAttribute
        (0xD9EF8236, 32, 0x33, 0, 0, 0x00000000), // bbMin
        (0xE78AA618, 48, 0x33, 0, 0, 0x00000000), // bbMax
        (0xD61872FF, 64, 0x33, 0, 0, 0x00000000), // bsCentre
        (0x82E8C5DE, 80, 0x21, 0, 0, 0x00000000), // bsRadius
        (0xAD5D5B4C, 84, 0x21, 0, 0, 0x00000000), // hdTextureDist
        (0xE6E17F14, 88, 0x4A, 0, 0, 0x00000000), // name
        (0x75D215A1, 92, 0x4A, 0, 0, 0x00000000), // textureDictionary
        (0x19471791, 96, 0x4A, 0, 0, 0x00000000), // clipDictionary
        (0xFC1FB6FB, 100, 0x4A, 0, 0, 0x00000000), // drawableDictionary
        (0xD3C717FC, 104, 0x4A, 0, 0, 0x00000000), // physicsDictionary
        (0x069004B2, 108, 0x62, 0, 0, 0x76BAF7C7), // assetType rage__fwArchetypeDef__eAssetType
        (0x9A77C7C5, 112, 0x4A, 0, 0, 0x00000000), // assetName
        (0x00000100, 0, 0x07, 0, 0, 0x00000000), // (array element)
        (0xF17E7F28, 120, 0x52, 0, 17, 0x00000000), // extensions
        (0x9A7B6E6D, 136, 0x17, 0, 0, 0x00000000), // guid
        (0x07D164A8, 144, 0x62, 0, 0, 0x079525DA), // 0x07D164A8 MapEntityType
        (0xD607DE68, 160, 0x15, 0, 0, 0x00000000), // mloFlags
        (0x00000100, 0, 0x07, 0, 0, 0x00000000), // (array element)
        (0xCCAB9307, 168, 0x52, 0, 22, 0x00000000), // entities
        (0x00000100, 0, 0x05, 0, 0, 0x0B1811F1), // (array element) CMloRoomDef
        (0xCD20D7A8, 184, 0x52, 0, 24, 0x00000000), // rooms
        (0x00000100, 0, 0x05, 0, 0, 0x995072CA), // (array element) CMloPortalDef
        (0x89F7E992, 200, 0x52, 0, 26, 0x00000000), // portals
        (0x00000100, 0, 0x05, 0, 0, 0xD6A799F9), // (array element) CMloEntitySet
        (0x45BCB930, 216, 0x52, 0, 28, 0x00000000), // entitySets
        (0x00000100, 0, 0x05, 0, 0, 0x301D99A8), // (array element) CMloTimeCycleModifier
        (0xAF9C3BD9, 232, 0x52, 0, 30, 0x00000000), // timeCycleModifiers
        (0x00000100, 0, 0x05, 0, 0, 0x4EB04539), // (array element)
        (0x8471928B, 248, 0x52, 0, 32, 0x00000000), // 0x8471928B
        (0x00000100, 0, 0x05, 0, 0, 0xFB36133A), // (array element)
        (0xEC8E962B, 264, 0x52, 0, 34, 0x00000000), // 0xEC8E962B
        (0x2AF2E200, 288, 0x05, 0, 0, 0xFB36133A), // 0x2AF2E200
        (0xA916F55B, 336, 0x01, 0, 0, 0x00000000), // 0xA916F55B
    ],
};

/// CMloEntitySet (0xD6A799F9), 48 bytes, 4 carrier files
pub const CMLOENTITYSET: ClassDef = ClassDef {
    name: 0xD6A799F9, key: 2337122009, unk8: 768, size: 48,
    entries: &[
        (0xE6E17F14, 8, 0x4A, 0, 0, 0x00000000), // name
        (0x00000100, 0, 0x15, 0, 0, 0x00000000), // (array element)
        (0xDDFC2211, 16, 0x52, 0, 1, 0x00000000), // locations
        (0x00000100, 0, 0x07, 0, 0, 0x00000000), // (array element)
        (0xCCAB9307, 32, 0x52, 0, 3, 0x00000000), // entities
    ],
};

/// CMloInstanceDef (0x09CC28BE), 192 bytes, 6 carrier files
pub const CMLOINSTANCEDEF: ClassDef = ClassDef {
    name: 0x09CC28BE, key: 1737908903, unk8: 1024, size: 192,
    entries: &[
        (0xA023A02C, 8, 0x4A, 0, 0, 0x00000000), // archetypeName
        (0x67D26872, 12, 0x15, 0, 0, 0x00000000), // flags
        (0x1B60404D, 16, 0x08, 0, 0, 0x00000000), // id
        (0x01166164, 32, 0x33, 0, 0, 0x00000000), // position
        (0x77DD4010, 48, 0x34, 0, 0, 0x00000000), // rotation
        (0x9CA32637, 64, 0x21, 0, 0, 0x00000000), // scaleXY
        (0x10FB7C42, 68, 0x21, 0, 0, 0x00000000), // scaleZ
        (0xD89231BD, 72, 0x14, 0, 0, 0x00000000), // parentIndex
        (0xBF74DFA7, 76, 0x21, 0, 0, 0x00000000), // lodDist
        (0xCA974BCD, 80, 0x21, 0, 0, 0x00000000), // childLodDist
        (0x6C8F1715, 84, 0x62, 0, 0, 0x4B5ACC2F), // lodLevel rage__eLodType
        (0xA687AC89, 88, 0x15, 0, 0, 0x00000000), // numChildren
        (0x2691F019, 92, 0x62, 0, 0, 0x26A60207), // priorityLevel rage__ePriorityLevel
        (0x00000100, 0, 0x07, 0, 0, 0x00000000), // (array element)
        (0xF17E7F28, 96, 0x52, 0, 13, 0x00000000), // extensions
        (0xFFEC781A, 112, 0x15, 0, 0, 0x00000000), // powerGridId
        (0x3C852527, 128, 0x15, 0, 0, 0x00000000), // tintValue
        (0x0DC5F270, 132, 0x11, 0, 0, 0x00000000), // blendAgeLayer
        (0x540CA436, 133, 0x11, 0, 0, 0x00000000), // blendAgeDirt
        (0x951BDD14, 144, 0x62, 0, 0, 0x66FBE3E0), // groupId eInteriorGroupId
        (0x8264E631, 148, 0x15, 0, 0, 0x00000000), // floorId
        (0x00000100, 0, 0x4A, 0, 0, 0x00000000), // (array element)
        (0x53DF8649, 152, 0x52, 0, 21, 0x00000000), // defaultEntitySets
        (0x1F837FB7, 168, 0x15, 0, 0, 0x00000000), // numExitPortals
        (0xE03B0CAA, 172, 0x15, 0, 0, 0x00000000), // MLOInstflags
        (0x15810E7F, 176, 0x21, 0, 0, 0x00000000), // 0x15810E7F
    ],
};

/// CMloPortalDef (0x995072CA), 80 bytes, 8 carrier files
pub const CMLOPORTALDEF: ClassDef = ClassDef {
    name: 0x995072CA, key: 1695312579, unk8: 768, size: 80,
    entries: &[
        (0xF470D2FD, 8, 0x15, 0, 0, 0x00000000), // roomFrom
        (0x9B649621, 12, 0x15, 0, 0, 0x00000000), // roomTo
        (0x67D26872, 16, 0x15, 0, 0, 0x00000000), // flags
        (0x46A92719, 20, 0x15, 0, 0, 0x00000000), // mirrorPriority
        (0xD9394F28, 24, 0x15, 0, 0, 0x00000000), // opacity
        (0x7C096C2E, 28, 0x21, 0, 0, 0x00000000), // 0x7C096C2E
        (0x91171666, 32, 0x21, 0, 0, 0x00000000), // 0x91171666
        (0x96816DDF, 36, 0x21, 0, 0, 0x00000000), // 0x96816DDF
        (0x0854FDEE, 40, 0x21, 0, 0, 0x00000000), // 0x0854FDEE
        (0x4131E934, 44, 0x15, 0, 0, 0x00000000), // audioOcclusion
        (0x00000100, 0, 0x33, 0, 0, 0x00000000), // (array element)
        (0x4F42AEB3, 48, 0x52, 0, 10, 0x00000000), // corners
        (0x00000100, 0, 0x15, 0, 0, 0x00000000), // (array element)
        (0x8E05312C, 64, 0x52, 0, 12, 0x00000000), // attachedObjects
    ],
};

/// CMloRoomDef (0x0B1811F1), 128 bytes, 8 carrier files
pub const CMLOROOMDEF: ClassDef = ClassDef {
    name: 0x0B1811F1, key: 3106284262, unk8: 1024, size: 128,
    entries: &[
        (0xE6E17F14, 8, 0x44, 0, 0, 0x00000000), // name
        (0xD9EF8236, 32, 0x33, 0, 0, 0x00000000), // bbMin
        (0xE78AA618, 48, 0x33, 0, 0, 0x00000000), // bbMax
        (0xEA73A60B, 64, 0x21, 0, 0, 0x00000000), // blend
        (0xA261E0A9, 68, 0x4A, 0, 0, 0x00000000), // timecycleName
        (0xC208509C, 72, 0x4A, 0, 0, 0x00000000), // secondaryTimecycleName
        (0x67D26872, 76, 0x15, 0, 0, 0x00000000), // flags
        (0x41E225B3, 80, 0x15, 0, 0, 0x00000000), // portalCount
        (0x8264E631, 84, 0x14, 0, 0, 0x00000000), // floorId
        (0x20F3D23E, 88, 0x14, 0, 0, 0x00000000), // exteriorVisibiltyDepth
        (0x00000100, 0, 0x15, 0, 0, 0x00000000), // (array element)
        (0x8E05312C, 96, 0x52, 0, 10, 0x00000000), // attachedObjects
        (0x00000100, 0, 0x05, 0, 0, 0x7BEF699E), // (array element) CReflectionProbeData
        (0x4B496F93, 112, 0x52, 0, 12, 0x00000000), // reflectionProbes
    ],
};

/// CReflectionProbeData (0x7BEF699E), 112 bytes, 6 carrier files
pub const CREFLECTIONPROBEDATA: ClassDef = ClassDef {
    name: 0x7BEF699E, key: 2026785216, unk8: 1024, size: 112,
    entries: &[
        (0x672D4771, 16, 0x33, 0, 0, 0x00000000), // minExtents
        (0x98474238, 32, 0x33, 0, 0, 0x00000000), // maxExtents
        (0x77DD4010, 48, 0x34, 0, 0, 0x00000000), // rotation
        (0xEA55969B, 64, 0x33, 0, 0, 0x00000000), // centerOffset
        (0x0ED751B9, 80, 0x33, 0, 0, 0x00000000), // influenceExtents
        (0xB835B0FA, 96, 0x11, 0, 0, 0x00000000), // probePriority
        (0x9A7B6E6D, 104, 0x08, 0, 0, 0x00000000), // guid
    ],
};

/// rage__fwInstancedMapData (0xF14A15BE), 48 bytes, 14 carrier files
pub const RAGE_FWINSTANCEDMAPDATA: ClassDef = ClassDef {
    name: 0xF14A15BE, key: 277409671, unk8: 768, size: 48,
    entries: &[
        (0x7FAE45E2, 8, 0x4A, 0, 0, 0x00000000), // ImapLink
        (0x00000100, 0, 0x05, 0, 0, 0xBA049770), // (array element) rage__fwPropInstanceListDef
        (0xD3AF3360, 16, 0x52, 0, 1, 0x00000000), // PropInstanceList
        (0x00000100, 0, 0x05, 0, 0, 0x7C475B5D), // (array element) rage__fwGrassInstanceListDef
        (0x0F3773DD, 32, 0x52, 0, 3, 0x00000000), // GrassInstanceList
    ],
};

/// rage__spdSphere (0x3F4F4469), 16 bytes, 8 carrier files
pub const RAGE_SPDSPHERE: ClassDef = ClassDef {
    name: 0x3F4F4469, key: 2399254030, unk8: 1024, size: 16,
    entries: &[
        (0xF3C49301, 0, 0x34, 0, 0, 0x00000000), // centerAndRadius
    ],
};

/// enum 0x9F874F86 (0x9F874F86)
pub const E_9F874F86: EnumDef = EnumDef {
    name: 0x9F874F86, key: 426447937,
    values: &[(0, 0xEC7602A4), (1, 0x9E49A27B), (2, 0xB4E2F547)],
};

/// enum CExtensionDefLadderMaterialType (0x4D24FF09)
pub const E_CEXTENSIONDEFLADDERMATERIALTYPE: EnumDef = EnumDef {
    name: 0x4D24FF09, key: 233076000,
    values: &[(0, 0x41AC1894), (1, 0x3271FA74), (2, 0xBEE41060)],
};

/// enum MapEntityType (0x079525DA)
pub const E_MAPENTITYTYPE: EnumDef = EnumDef {
    name: 0x079525DA, key: 3087975696,
    values: &[(0, 0x9B931532), (1, 0xAD1EED63), (2, 0x19341C87), (3, 0x7CD164E4), (4, 0x70069A13), (5, 0x2BE5C1FB), (6, 0x65D4F74A), (7, 0x830170E5)],
};

/// enum eInteriorGroupId (0x66FBE3E0)
pub const E_EINTERIORGROUPID: EnumDef = EnumDef {
    name: 0x66FBE3E0, key: 1534468459,
    values: &[(0, 0xA00FBCC5), (1, 0xEC89C407), (2, 0xC7250764), (3, 0xB0D46756), (5, 0x08172B20), (1243632011, 0xA5A56E2F), (1343859736, 0x809FF313), (3920959586, 0x4A02E7BB)],
};

/// enum rage__eLodType (0x4B5ACC2F)
pub const E_RAGE_ELODTYPE: EnumDef = EnumDef {
    name: 0x4B5ACC2F, key: 1458091376,
    values: &[(0, 0xC2485BBC), (1, 0x8E22C856), (2, 0x983C176C), (3, 0x12CD0C94), (4, 0x81046901), (5, 0x7D934F41), (6, 0x6F5D45B3)],
};

/// enum rage__ePriorityLevel (0x26A60207)
pub const E_RAGE_EPRIORITYLEVEL: EnumDef = EnumDef {
    name: 0x26A60207, key: 1627350673,
    values: &[(0, 0x73D556CB), (1, 0xEE09C197), (2, 0x1EBB6975), (3, 0x13A5B7D4)],
};

/// enum rage__fwArchetypeDef__eAssetType (0x76BAF7C7)
pub const E_RAGE_FWARCHETYPEDEF_EASSETTYPE: EnumDef = EnumDef {
    name: 0x76BAF7C7, key: 1561968235,
    values: &[(0, 0x0B4F1FED), (1, 0x22097FE7), (2, 0x07C0CB71), (3, 0x5E2F6A14), (4, 0xF8051E71)],
};

/// Every class, for lookup by name hash.
pub const CLASSES: &[&ClassDef] = &[&C_A11E83E5, &C_A15F529D, &C_E5B7A9F2, &C_FB36133A, &CBASEARCHETYPEDEF, &CBLOCKDESC, &CDISTANTLODLIGHT, &CENTITYDEF, &CEXTENSIONDEFAUDIOCOLLISIONSETTINGS, &CEXTENSIONDEFDOOR, &CEXTENSIONDEFLADDER, &CEXTENSIONDEFLIGHTEFFECT, &CEXTENSIONDEFSTAIRS, &CLODLIGHT, &CLIGHTATTRDEF, &CMAPDATA, &CMAPTYPES, &CMLOARCHETYPEDEF, &CMLOENTITYSET, &CMLOINSTANCEDEF, &CMLOPORTALDEF, &CMLOROOMDEF, &CREFLECTIONPROBEDATA, &RAGE_FWINSTANCEDMAPDATA, &RAGE_SPDSPHERE];
/// Every enum, for lookup by name hash.
pub const ENUMS: &[&EnumDef] = &[&E_9F874F86, &E_CEXTENSIONDEFLADDERMATERIALTYPE, &E_MAPENTITYTYPE, &E_EINTERIORGROUPID, &E_RAGE_ELODTYPE, &E_RAGE_EPRIORITYLEVEL, &E_RAGE_FWARCHETYPEDEF_EASSETTYPE];
