use super::union::GenUnion;
use crate::ast::verified::{self as ast, HasName};

use case::CaseExt;
use std::io;

pub(super) trait Generator: HasName {
    fn generate<W: io::Write>(&self, writer: &mut W) -> io::Result<()>;
    fn common_generate<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        let struct_name = self.name().to_camel();

        let define = format!(
            r#"
            type {struct_name} struct {{
                inner []byte
            }}
        "#,
            struct_name = struct_name
        );
        writeln!(writer, "{}", define)?;

        let impl_ = format!(
            r#"
            func {struct_name}FromSliceUnchecked(slice []byte) *{struct_name} {{
                return &{struct_name}{{inner: slice}}
            }}
            func (s *{struct_name}) AsSlice() []byte {{
                return s.inner
            }}
            "#,
            struct_name = struct_name
        );
        writeln!(writer, "{}", impl_)?;
        Ok(())
    }
}

impl Generator for ast::Option_ {
    fn generate<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        self.common_generate(writer)?;

        let struct_name = self.name().to_camel();
        let inner = self.typ.name().to_camel();

        let constructor = format!(
            r#"
            func New{struct_name}WithData(v {inner_type}) {struct_name} {{
                return {struct_name}{{inner: v.AsSlice()}}
            }}

            func New{struct_name}() {struct_name} {{
                return {struct_name}{{inner: []byte{{}}}}
            }}
            func {struct_name}FromSlice(slice []byte, compatible bool) (*{struct_name}, error) {{
                if len(slice) == 0 {{
                    return &{struct_name}{{inner: slice}}, nil
                }}

                _, err := {inner_type}FromSlice(slice, compatible)
                if err != nil {{
                    return nil, err
                }}
                return &{struct_name}{{inner: slice}}, nil
            }}
            "#,
            struct_name = struct_name,
            inner_type = inner
        );
        writeln!(writer, "{}", constructor)?;

        let impl_ = format!(
            r#"
            func (s *{struct_name}) isSome() bool {{
                return len(s.inner) != 0
            }}

            func (s *{struct_name}) isNone() bool {{
                return len(s.inner) == 0
            }}
            "#,
            struct_name = struct_name
        );
        writeln!(writer, "{}", impl_)?;
        Ok(())
    }
}

impl Generator for ast::Union {
    fn generate<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        self.common_generate(writer)?;
        let struct_name = self.name().to_camel();
        let union_name = format!("{}Union", struct_name);

        let (union_impl, from_slice_switch_iml) = self.gen_union();
        writeln!(writer, "{}", union_impl)?;

        let struct_constructor = format!(
            r#"
            func New{struct_name}(v {union_name}) {struct_name} {{
                s := new(bytes.Buffer)
                s.Write(packNumber(v.itemID))
                s.Write(v.AsSlice())

                return {struct_name}{{inner: s.Bytes()}}
            }}
            func {struct_name}FromSlice(slice []byte, compatible bool) (*{struct_name}, error) {{
                sliceLen := len(slice)
                if uint32(sliceLen) < HeaderSizeUint {{
                    errMsg := strings.Join([]string{{"HeaderIsBroken", "{struct_name}", strconv.Itoa(int(sliceLen)), "<", strconv.Itoa(int(HeaderSizeUint))}}, " ")
                    return nil, errors.New(errMsg)
                }}
                itemID := unpackNumber(slice)
                innerSlice := slice[HeaderSizeUint:]

                switch itemID {{
                {from_slice_switch_iml}
                default:
                    return nil, errors.New("UnknownItem, {struct_name}")
                }}
                return &{struct_name}{{inner: slice}}, nil
            }}
            "#,
            struct_name = struct_name,
            union_name = union_name,
            from_slice_switch_iml = from_slice_switch_iml
        );
        writeln!(writer, "{}", struct_constructor)?;

        let struct_impl = format!(
            r#"
            func (s *{}) ItemID() Number {{
                return unpackNumber(s.inner)
            }}
            "#,
            struct_name
        );
        writeln!(writer, "{}", struct_impl)?;
        Ok(())
    }
}

impl Generator for ast::Array {
    fn generate<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        let struct_name = self.name().to_camel();
        let inner = self.typ.name().to_camel();
        let item_count = self.item_count;
        let total_size = self.total_size();

        self.common_generate(writer)?;

        let impl_ = format!(
            r#"
            func New{struct_name}(array [{item_count}]{inner_type}) {struct_name} {{
                s := new(bytes.Buffer)
                len := len(array)
                for i := 0; i < len; i++ {{
                    s.Write(array[i].AsSlice())
                }}
                return {struct_name}{{inner: s.Bytes()}}
            }}

            func {struct_name}FromSlice(slice []byte, _compatible bool) (*{struct_name}, error) {{
                sliceLen := len(slice)
                if sliceLen != {total_size} {{
                    errMsg := strings.Join([]string{{"TotalSizeNotMatch", "{struct_name}", strconv.Itoa(int(sliceLen)), "!=", strconv.Itoa({total_size})}}, " ")
                    return nil, errors.New(errMsg)
                }}
                return &{struct_name}{{inner: slice}}, nil
            }}
        "#,
            struct_name = struct_name,
            inner_type = inner,
            item_count = item_count,
            total_size = total_size
        );
        writeln!(writer, "{}", impl_)?;

        if self.typ.is_atom() {
            writeln!(
                writer,
                r#"
            func (s *{struct_name}) RawData() []byte {{
                return s.inner
            }}
            "#,
                struct_name = struct_name
            )?
        }

        for i in 0..self.item_count {
            let func_name = format!("Nth{}", i);
            let start = self.item_size * i;
            let end = self.item_size * (i + 1);

            writeln!(
                writer,
                r#"
            func (s *{struct_name}) {func_name}() *{inner_type} {{
                ret := {inner_type}FromSliceUnchecked(s.inner[{start}:{end}])
                return ret
            }}
            "#,
                struct_name = struct_name,
                func_name = func_name,
                inner_type = inner,
                start = start,
                end = end
            )?
        }

        Ok(())
    }
}

impl Generator for ast::Struct {
    fn generate<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        let struct_name = self.name().to_camel();
        let total_size = self.total_size();

        self.common_generate(writer)?;

        let fields_param = self
            .inner
            .iter()
            .map(|f| {
                let field_name = &f.name;
                let field_type = f.typ.name().to_camel();
                format!("{} {}", field_name, field_type)
            })
            .collect::<Vec<String>>()
            .join(", ");

        let fields_encode = self
            .inner
            .iter()
            .map(|f| {
                let field_name = &f.name;
                format!("s.Write({}.AsSlice())", field_name)
            })
            .collect::<Vec<String>>()
            .join("\n");

        let impl_ = format!(
            r#"
            func New{struct_name}({fields_param}) {struct_name} {{
                s := new(bytes.Buffer)
                {fields_encode}
                return {struct_name}{{inner: s.Bytes()}}
            }}

            func {struct_name}FromSlice(slice []byte, _compatible bool) (*{struct_name}, error) {{
                sliceLen := len(slice)
                if sliceLen != {total_size} {{
                    errMsg := strings.Join([]string{{"TotalSizeNotMatch", "{struct_name}", strconv.Itoa(int(sliceLen)), "!=", strconv.Itoa({total_size})}}, " ")
                    return nil, errors.New(errMsg)
                }}
                return &{struct_name}{{inner: slice}}, nil
            }}
        "#,
            struct_name = struct_name,
            fields_param = fields_param,
            fields_encode = fields_encode,
            total_size = total_size
        );
        writeln!(writer, "{}", impl_)?;

        let (_, each_getter) = self.inner.iter().zip(self.field_size.iter()).fold(
            (0, Vec::with_capacity(self.inner.len())),
            |(mut offset, mut getters), (f, s)| {
                let func_name = f.name.to_camel();
                let inner = f.typ.name().to_camel();

                let start = offset;
                offset += s;
                let end = offset;
                let getter = format!(
                    r#"
                    func (s *{struct_name}) {func_name}() *{inner} {{
                        ret := {inner}FromSliceUnchecked(s.inner[{start}:{end}])
                        return ret
                    }}
                "#,
                    struct_name = struct_name,
                    inner = inner,
                    start = start,
                    end = end,
                    func_name = func_name
                );

                getters.push(getter);
                (offset, getters)
            },
        );

        writeln!(writer, "{}", each_getter.join("\n"))?;

        Ok(())
    }
}

impl Generator for ast::FixVec {
    fn generate<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        let struct_name = self.name().to_camel();
        let inner = self.typ.name().to_camel();
        let item_size = self.item_size;

        self.common_generate(writer)?;

        let constructor = format!(
            r#"
            func New{struct_name}(vec []{inner_type}) {struct_name} {{
                size := packNumber(Number(len(vec)))

                s := new(bytes.Buffer)

                s.Write(size)
                len := len(vec)
                for i := 0; i < len; i++ {{
                    s.Write(vec[i].AsSlice())
                }}

                sb := {struct_name}{{inner: s.Bytes()}}

                return sb
            }}
            func {struct_name}FromSlice(slice []byte, _compatible bool) (*{struct_name}, error) {{
                sliceLen := len(slice)
                if sliceLen < int(HeaderSizeUint) {{
                    errMsg := strings.Join([]string{{"HeaderIsBroken", "{struct_name}", strconv.Itoa(int(sliceLen)), "<", strconv.Itoa(int(HeaderSizeUint))}}, " ")
                    return nil, errors.New(errMsg)
                }}
                itemCount := unpackNumber(slice)
                if itemCount == 0 {{
                    if sliceLen != int(HeaderSizeUint) {{
                        errMsg := strings.Join([]string{{"TotalSizeNotMatch", "{struct_name}", strconv.Itoa(int(sliceLen)), "!=", strconv.Itoa(int(HeaderSizeUint))}}, " ")
                        return nil, errors.New(errMsg)
                    }}
                    return &{struct_name}{{inner: slice}}, nil
                }}
                totalSize := int(HeaderSizeUint) + int({item_size}*itemCount)
                if sliceLen != totalSize {{
                    errMsg := strings.Join([]string{{"TotalSizeNotMatch", "{struct_name}", strconv.Itoa(int(sliceLen)), "!=", strconv.Itoa(int(totalSize))}}, " ")
                    return nil, errors.New(errMsg)
                }}
                return &{struct_name}{{inner: slice}}, nil
            }}
            "#,
            struct_name = struct_name,
            inner_type = inner,
            item_size = item_size
        );
        writeln!(writer, "{}", constructor)?;

        let impl_ = format!(
            r#"
            func (s *{struct_name}) TotalSize() uint {{
                return uint(HeaderSizeUint) * (s.ItemCount()+1)
            }}
            func (s *{struct_name}) ItemCount() uint {{
                number := uint(unpackNumber(s.inner))
                return number
            }}
            func (s *{struct_name}) Len() uint {{
                return s.ItemCount()
            }}
            func (s *{struct_name}) IsEmpty() bool {{
                return s.Len() == 0
            }}
            // if *{inner_type} is nil, index is out of bounds
            func (s *{struct_name}) Get(index uint) *{inner_type} {{
                var re *{inner_type}
                if index < s.Len() {{
                    start := uint(HeaderSizeUint) + {item_size}*index
                    end := start + {item_size}
                    re = {inner_type}FromSliceUnchecked(s.inner[start:end])
                }}
                return re
            }}
        "#,
            struct_name = struct_name,
            inner_type = inner,
            item_size = item_size
        );
        writeln!(writer, "{}", impl_)?;

        if self.typ.is_atom() {
            writeln!(
                writer,
                r#"
            func (s *{struct_name}) RawData() []byte {{
                return s.inner[HeaderSizeUint:]
            }}
            "#,
                struct_name = struct_name
            )?
        }
        Ok(())
    }
}

impl Generator for ast::DynVec {
    fn generate<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        let struct_name = self.name().to_camel();
        let inner = self.typ.name().to_camel();

        self.common_generate(writer)?;

        let constructor = format!(
            r#"
            func New{struct_name}(vec []{inner_type}) {struct_name} {{
                itemCount := len(vec)
                size := packNumber(Number(itemCount))

                s := new(bytes.Buffer)

                // Empty dyn vector, just return size's bytes
                if itemCount == 0 {{
                    s.Write(size)
                    return {struct_name}{{inner: s.Bytes()}}
                }}

                // Calculate first offset then loop for rest items offsets
                totalSize := HeaderSizeUint * uint32(itemCount+1)
                offsets := make([]uint32, 0, itemCount)
                offsets = append(offsets, totalSize)
                for i := 1; i < itemCount; i++ {{
                    totalSize += uint32(len(vec[i-1].AsSlice()))
                    offsets = append(offsets, offsets[i-1]+uint32(len(vec[i-1].AsSlice())))
                }}
                totalSize += uint32(len(vec[itemCount-1].AsSlice()))

                s.Write(packNumber(Number(totalSize)))

                for i := 0; i < itemCount; i++ {{
                    s.Write(packNumber(Number(offsets[i])))
                }}

                for i := 0; i < itemCount; i++ {{
                    s.Write(vec[i].AsSlice())
                }}

                return {struct_name}{{inner: s.Bytes()}}
            }}
            func {struct_name}FromSlice(slice []byte, compatible bool) (*{struct_name}, error) {{
                sliceLen := len(slice)

                if uint32(sliceLen) < HeaderSizeUint {{
                    errMsg := strings.Join([]string{{"HeaderIsBroken", "{struct_name}", strconv.Itoa(int(sliceLen)), "<", strconv.Itoa(int(HeaderSizeUint))}}, " ")
                    return nil, errors.New(errMsg)
                }}

                totalSize := unpackNumber(slice)
                if Number(sliceLen) != totalSize {{
                    errMsg := strings.Join([]string{{"TotalSizeNotMatch", "{struct_name}", strconv.Itoa(int(sliceLen)), "!=", strconv.Itoa(int(totalSize))}}, " ")
                    return nil, errors.New(errMsg)
                }}

                if uint32(sliceLen) == HeaderSizeUint {{
                    return &{struct_name}{{inner: slice}}, nil
                }}

                if uint32(sliceLen) < HeaderSizeUint*2 {{
                    errMsg := strings.Join([]string{{"TotalSizeNotMatch", "{struct_name}", strconv.Itoa(int(sliceLen)), "<", strconv.Itoa(int(HeaderSizeUint*2))}}, " ")
                    return nil, errors.New(errMsg)
                }}

                offsetFirst := unpackNumber(slice[HeaderSizeUint:])
                if offsetFirst%4 != 0 || uint32(offsetFirst) < HeaderSizeUint*2 {{
                    errMsg := strings.Join([]string{{"OffsetsNotMatch", "{struct_name}", strconv.Itoa(int(offsetFirst%4)), "!= 0", strconv.Itoa(int(offsetFirst)), "<", strconv.Itoa(int(HeaderSizeUint*2))}}, " ")
                    return nil, errors.New(errMsg)
                }}

                itemCount := offsetFirst/4 - 1
                headerSize := HeaderSizeUint * (uint32(itemCount) + 1)
                if uint32(sliceLen) < headerSize {{
                    errMsg := strings.Join([]string{{"HeaderIsBroken", "{struct_name}", strconv.Itoa(int(sliceLen)), "<", strconv.Itoa(int(headerSize))}}, " ")
                    return nil, errors.New(errMsg)
                }}

                offsets := make([]uint32, itemCount)

                for i := 0; i < int(itemCount); i++ {{
                    offsets[i] = uint32(unpackNumber(slice[HeaderSizeUint:][4*i:]))
                }}

                offsets = append(offsets, uint32(totalSize))

                for i := 0; i < len(offsets); i++ {{
                    if i&1 != 0 && offsets[i-1] > offsets[i] {{
                        errMsg := strings.Join([]string{{"OffsetsNotMatch", "{struct_name}"}}, " ")
                        return nil, errors.New(errMsg)
                    }}
                }}

                for i := 0; i < len(offsets); i++ {{
                    if i&1 != 0 {{
                        start := offsets[i-1]
                        end := offsets[i]
                        _, err := {inner_type}FromSlice(slice[start:end], compatible)

                        if err != nil {{
                            return nil, err
                        }}
                    }}
                }}

                return &{struct_name}{{inner: slice}}, nil
            }}
            "#,
            struct_name = struct_name,
            inner_type = inner
        );
        writeln!(writer, "{}", constructor)?;

        let impl_ = format!(
            r#"
            func (s *{struct_name}) TotalSize() uint {{
                return uint(unpackNumber(s.inner))
            }}
            func (s *{struct_name}) ItemCount() uint {{
                var number uint = 0
                if uint32(s.TotalSize()) == HeaderSizeUint {{
                    return number
                }}
                number = uint(unpackNumber(s.inner[HeaderSizeUint:]))/4 - 1
                return number
            }}
            func (s *{struct_name}) Len() uint {{
                return s.ItemCount()
            }}
            func (s *{struct_name}) IsEmpty() bool {{
                return s.Len() == 0
            }}
            func (s *{struct_name}) itemOffsets() [][4]byte {{
                // Preventing index out-of-bounds array accesses when not alignment
                dataSize := len(s.inner[HeaderSizeUint:]) - len(s.inner[HeaderSizeUint:])%4
                cap := dataSize / 4
                ret := make([][4]byte, cap)
                var firstIdx, secondIdex int
                for i := 0; i < dataSize; i++ {{
                    firstIdx = i >> 2
                    if firstIdx > cap {{
                        break
                    }}
                    secondIdex = i % 4
                    ret[firstIdx][secondIdex] = s.inner[HeaderSizeUint:][firstIdx*4:][secondIdex]
                }}
                return ret
            }}
            // if *{inner_type} is nil, index is out of bounds
            func (s *{struct_name}) Get(index uint) *{inner_type} {{
                var b *{inner_type}
                if index < s.Len() {{
                    offsets := s.itemOffsets()
                    start := unpackNumber(offsets[index][:])

                    if index == s.Len()-1 {{
                        b = {inner_type}FromSliceUnchecked(s.inner[start:])
                    }} else {{
                        end := unpackNumber(offsets[index+1][:])
                        b = {inner_type}FromSliceUnchecked(s.inner[start:end])
                    }}
                }}
                return b
            }}
            "#,
            struct_name = struct_name,
            inner_type = inner
        );
        writeln!(writer, "{}", impl_)?;
        Ok(())
    }
}

impl Generator for ast::Table {
    fn generate<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        let field_count = self.inner.len();
        let struct_name = self.name().to_camel();

        self.common_generate(writer)?;

        let constructor = if self.inner.is_empty() {
            format!(
                r#"
            func New{struct_name}() {struct_name} {{
                s := new(bytes.Buffer)
                s.Write(packNumber(Number(HeaderSizeUint)))
            }}
            func {struct_name}FromSlice(slice []byte, compatible bool) (*{struct_name}, error) {{
                sliceLen := len(slice)
                if uint32(sliceLen) < HeaderSizeUint {{
                    return nil, errors.New("HeaderIsBroken")
                }}

                totalSize := unpackNumber(slice)
                if Number(sliceLen) != totalSize {{
                    return nil, errors.New("TotalSizeNotMatch")
                }}

                if uint32(sliceLen) > HeaderSizeUint && !compatible {{
                    return nil, errors.New("FieldCountNotMatch")
                }}
                return &{struct_name}{{inner: slice}}, nil
            }}
            "#,
                struct_name = struct_name
            )
        } else {
            let fields_param = self
                .inner
                .iter()
                .map(|f| {
                    let field_name = &f.name;
                    let field_type = f.typ.name().to_camel();
                    format!("{} {}", field_name, field_type)
                })
                .collect::<Vec<String>>()
                .join(", ");

            let fields_offset = self
                .inner
                .iter()
                .map(|f| {
                    let field_name = &f.name;
                    format!("offsets = append(offsets, totalSize)\ntotalSize += uint32(len({}.AsSlice()))", field_name)
                })
                .collect::<Vec<String>>()
                .join("\n");

            let fields_encode = self
                .inner
                .iter()
                .map(|f| {
                    let field_name = &f.name;
                    format!("s.Write({}.AsSlice())", field_name)
                })
                .collect::<Vec<String>>()
                .join("\n");

            let verify_fields = self
                .inner
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let field = f.typ.name().to_camel();
                    let start = i;
                    let end = i + 1;
                    format!(
                        r#"
                    _, err := {field}FromSlice(slice[offsets[{start}]:offsets[{end}]], compatible)
                    if err != nil {{
                        return nil, err
                    }}
                "#,
                        field = field,
                        start = start,
                        end = end
                    )
                })
                .collect::<Vec<String>>()
                .join("\n");

            format!(
                r#"
            func New{struct_name}({fields_param}) {struct_name} {{
                s := new(bytes.Buffer)

                totalSize := HeaderSizeUint * ({field_count} + 1)
                offsets := make([]uint32, 0, {field_count})

                {fields_offset}

                s.Write(packNumber(Number(totalSize)))

                for i := 0; i < len(offsets); i++ {{
                    s.Write(packNumber(Number(offsets[i])))
                }}

                {fields_encode}
                return {struct_name}{{inner: s.Bytes()}}
            }}
            func {struct_name}FromSlice(slice []byte, compatible bool) (*{struct_name}, error) {{
                sliceLen := len(slice)
                if uint32(sliceLen) < HeaderSizeUint {{
                    errMsg := strings.Join([]string{{"HeaderIsBroken", "{struct_name}", strconv.Itoa(int(sliceLen)), "<", strconv.Itoa(int(HeaderSizeUint))}}, " ")
                    return nil, errors.New(errMsg)
                }}

                totalSize := unpackNumber(slice)
                if Number(sliceLen) != totalSize {{
                    errMsg := strings.Join([]string{{"TotalSizeNotMatch", "{struct_name}", strconv.Itoa(int(sliceLen)), "!=", strconv.Itoa(int(totalSize))}}, " ")
                    return nil, errors.New(errMsg)
                }}

                if uint32(sliceLen) == HeaderSizeUint && {field_count} == 0 {{
                    return &{struct_name}{{inner: slice}}, nil
                }}

                if uint32(sliceLen) < HeaderSizeUint*2 {{
                    errMsg := strings.Join([]string{{"TotalSizeNotMatch", "{struct_name}", strconv.Itoa(int(sliceLen)), "<", strconv.Itoa(int(HeaderSizeUint*2))}}, " ")
                    return nil, errors.New(errMsg)
                }}

                offsetFirst := unpackNumber(slice[HeaderSizeUint:])
                if offsetFirst%4 != 0 || uint32(offsetFirst) < HeaderSizeUint*2 {{
                    errMsg := strings.Join([]string{{"OffsetsNotMatch", "{struct_name}", strconv.Itoa(int(offsetFirst%4)), "!= 0", strconv.Itoa(int(offsetFirst)), "<", strconv.Itoa(int(HeaderSizeUint*2))}}, " ")
                    return nil, errors.New(errMsg)
                }}

                fieldCount := offsetFirst/4 - 1
                if fieldCount < {field_count} {{
                    return nil, errors.New("FieldCountNotMatch")
                }} else if !compatible && fieldCount > {field_count} {{
                    return nil, errors.New("FieldCountNotMatch")
                }}

                headerSize := HeaderSizeUint * (uint32(fieldCount) + 1)
                if uint32(sliceLen) < headerSize {{
                    errMsg := strings.Join([]string{{"HeaderIsBroken", "{struct_name}", strconv.Itoa(int(sliceLen)), "<", strconv.Itoa(int(headerSize))}}, " ")
                    return nil, errors.New(errMsg)
                }}

                offsets := make([]uint32, {field_count})

                for i := 0; i < {field_count}; i++ {{
                    offsets[i] = uint32(unpackNumber(slice[HeaderSizeUint:][4*i:]))
                }}
                offsets = append(offsets, uint32(totalSize))

                for i := 0; i < len(offsets); i++ {{
                    if i&1 != 0 && offsets[i-1] > offsets[i] {{
                        return nil, errors.New("OffsetsNotMatch")
                    }}
                }}
                {verify_fields}

                return &{struct_name}{{inner: slice}}, nil
            }}
            "#,
                struct_name = struct_name,
                fields_param = fields_param,
                fields_offset = fields_offset,
                fields_encode = fields_encode,
                field_count = field_count,
                verify_fields = verify_fields
            )
        };
        writeln!(writer, "{}", constructor)?;

        let impl_ = format!(
            r#"
            func (s *{struct_name}) TotalSize() uint {{
                return uint(unpackNumber(s.inner))
            }}
            func (s *{struct_name}) FieldCount() uint {{
                var number uint = 0
                if uint32(s.TotalSize()) == HeaderSizeUint {{
                    return number
                }}
                number = uint(unpackNumber(s.inner[HeaderSizeUint:]))/4 - 1
                return number
            }}
            func (s *{struct_name}) Len() uint {{
                return s.FieldCount()
            }}
            func (s *{struct_name}) IsEmpty() bool {{
                return s.Len() == 0
            }}
            func (s *{struct_name}) FieldOffsets() [][4]byte {{
                // Preventing index out-of-bounds array accesses when not alignment
                dataSize := len(s.inner[HeaderSizeUint:]) - len(s.inner[HeaderSizeUint:])%4
                cap := dataSize / 4
                ret := make([][4]byte, cap)
                var firstIdx, secondIdex int
                for i := 0; i < dataSize; i++ {{
                    firstIdx = i >> 2
                    if firstIdx > cap {{
                        break
                    }}
                    secondIdex = i % 4
                    ret[firstIdx][secondIdex] = s.inner[HeaderSizeUint:][firstIdx*4:][secondIdex]
                }}
                return ret
            }}

            func (s *{struct_name}) CountExtraFields() uint {{
                return s.FieldCount() - {field_count}
            }}

            func (s *{struct_name}) hasExtraFields() bool {{
                return {field_count} != s.FieldCount()
            }}
            "#,
            struct_name = struct_name,
            field_count = field_count,
        );
        writeln!(writer, "{}", impl_)?;

        let (getter_stmt_last, getter_stmt) = {
            let getter_stmt_last = "s.inner[start:]".to_string();
            let getter_stmt = "s.inner[start:end]".to_string();
            (getter_stmt_last, getter_stmt)
        };
        let each_getter = self
            .inner
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let func = f.name.to_camel();

                let inner = f.typ.name().to_camel();
                let start = i;
                let end = i + 1;
                if i == self.inner.len() - 1 {
                    format!(
                        r#"
                        func (s *{struct_name}) {func}() *{inner} {{
                            var ret *{inner}
                            offsets := s.FieldOffsets()
                            start := unpackNumber(offsets[0][:])
                            if s.hasExtraFields() {{
                                end := unpackNumber(offsets[1][:])
                                ret = {inner}FromSliceUnchecked({getter_stmt})
                            }} else {{
                                ret = {inner}FromSliceUnchecked({getter_stmt_last})
                            }}
                            return ret
                        }}
                        "#,
                        struct_name = struct_name,
                        func = func,
                        inner = inner,
                        getter_stmt = getter_stmt,
                        getter_stmt_last = getter_stmt_last
                    )
                } else {
                    format!(
                        r#"
                        func (s *{struct_name}) {func}() *{inner} {{
                            offsets := s.FieldOffsets()
                            start := unpackNumber(offsets[{start}][:])
                            end := unpackNumber(offsets[{end}][:])
                            {inner}FromSliceUnchecked({getter_stmt})
                        }}
               "#,
                        struct_name = struct_name,
                        func = func,
                        inner = inner,
                        getter_stmt = getter_stmt,
                        start = start,
                        end = end
                    )
                }
            })
            .collect::<Vec<_>>();
        writeln!(writer, "{}", each_getter.join("\n"))?;
        Ok(())
    }
}
