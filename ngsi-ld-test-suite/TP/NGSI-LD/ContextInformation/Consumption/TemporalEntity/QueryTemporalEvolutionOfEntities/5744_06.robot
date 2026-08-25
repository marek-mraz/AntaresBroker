*** Settings ***
Documentation       Verify 4.9 values-filter edges on the temporal query
...                 that target the broker's extended filter compiler:
...                 string ordering (RFC 8259 code-unit comparison is the
...                 4.9 SHALL, UCA only a SHOULD), != against scalars,
...                 lists and array values (universal quantification,
...                 datatype-mismatch matches), and [lang]/[*] language
...                 filters. On pg/timescale CI cells these exercise the
...                 COLLATE "C" / NOT-of-Eq / languageMap prefilter
...                 leaves. Antares extension TP.

Library             RequestsLibrary
Library             Collections
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create Fixture Entities
Suite Teardown      Delete Fixture Entities


*** Variables ***
${mid}=         urn:ngsi-ld:Vehicle:qx5744-mm
${high}=        urn:ngsi-ld:Vehicle:qx5744-zz
${numname}=     urn:ngsi-ld:Vehicle:qx5744-num
${arr}=         urn:ngsi-ld:Vehicle:qx5744-arr
${lang}=        urn:ngsi-ld:Vehicle:qx5744-lang
${window}=      timerel=between&timeAt=2026-03-01T00:00:00Z&endTimeAt=2026-03-02T00:00:00Z


*** Test Cases ***
5744_06_01 String Ordering Is Code-Unit Comparison
    [Documentation]    4.9 (p.89): string order is RFC 8259 code-unit
    ...    comparison — name>"m" selects "z" but not "m" itself.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    name%3E%22m%22
    Should Contain    ${response.text}    ${high}
    Should Not Contain    ${response.text}    ${mid}
    Should Not Contain    ${response.text}    ${numname}

5744_06_02 String Ordering Includes The Boundary With Ge
    [Documentation]    4.9: name>="m" is inclusive — both "m" and "z"
    ...    match.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    name%3E%3D%22m%22
    Should Contain    ${response.text}    ${mid}
    Should Contain    ${response.text}    ${high}
    Should Not Contain    ${response.text}    ${numname}

5744_06_03 Ordering Above The Highest Value Matches Nothing
    [Documentation]    4.9 (p.89/p.91): name>"z" — code-unit order excludes
    ...    "m" and "z", and the NUMERIC name is a datatype mismatch, which
    ...    for ordering operators is not matching.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    name%3E%22z%22
    Should Not Contain    ${response.text}    qx5744

5744_06_04 Unequal Scalar
    [Documentation]    4.9 (p.92): != matches when values differ AND when
    ...    datatypes differ — the numeric name is unequal to a string.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    name%21%3D%22m%22
    Should Contain    ${response.text}    ${high}
    Should Contain    ${response.text}    ${numname}
    Should Not Contain    ${response.text}    ${mid}

5744_06_05 Unequal Against An Array Is Universal
    [Documentation]    4.9 (p.91): for an array target value, != requires
    ...    EVERY element to differ — tags!="a" fails for ["a","b"].
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    tags%21%3D%22c%22
    Should Contain    ${response.text}    ${arr}
    ${response}=    Query Temporal    tags%21%3D%22a%22
    Should Not Contain    ${response.text}    ${arr}

5744_06_06 Unequal Value List
    [Documentation]    4.9: != with a ValueList matches only values
    ...    different from ALL listed ones.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    name%21%3D%22m%22,%22z%22
    Should Contain    ${response.text}    ${numname}
    Should Not Contain    ${response.text}    ${mid}
    Should Not Contain    ${response.text}    ${high}

5744_06_07 Language Filter Specific Tag
    [Documentation]    4.9 (p.90): label[en]=="hello" targets the
    ...    languageMap entry — entities without a matching languageMap
    ...    value are excluded.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    label%5Ben%5D%3D%3D%22hello%22
    Should Contain    ${response.text}    ${lang}
    Should Not Contain    ${response.text}    ${mid}
    Should Not Contain    ${response.text}    ${arr}

5744_06_08 Language Filter Wildcard
    [Documentation]    4.9 (p.90): label[*] matches the value in ANY
    ...    language — the French-only value qualifies via [*] but not
    ...    via [en].
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    label%5B%2A%5D%3D%3D%22bonjour%22
    Should Contain    ${response.text}    ${lang}
    ${response}=    Query Temporal    label%5Ben%5D%3D%3D%22bonjour%22
    Should Not Contain    ${response.text}    ${lang}


*** Keywords ***
Query Temporal
    [Arguments]    ${q_expr}
    ${response}=    GET
    ...    url=${temporal_api_url}/temporal/entities
    ...    params=type=Vehicle&q=${q_expr}&${window}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    RETURN    ${response}

Create Temporal Entity
    [Arguments]    ${payload}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${temporal_api_url}/temporal/entities
    ...    data=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Create Fixture Entities
    Create Temporal Entity    {"id": "${mid}", "type": "Vehicle", "name": [{"type": "Property", "value": "m", "observedAt": "2026-03-01T12:00:00Z"}]}
    Create Temporal Entity    {"id": "${high}", "type": "Vehicle", "name": [{"type": "Property", "value": "z", "observedAt": "2026-03-01T12:00:00Z"}]}
    Create Temporal Entity    {"id": "${numname}", "type": "Vehicle", "name": [{"type": "Property", "value": 5, "observedAt": "2026-03-01T12:00:00Z"}]}
    Create Temporal Entity    {"id": "${arr}", "type": "Vehicle", "tags": [{"type": "Property", "value": ["a", "b"], "observedAt": "2026-03-01T12:00:00Z"}]}
    Create Temporal Entity    {"id": "${lang}", "type": "Vehicle", "label": [{"type": "LanguageProperty", "languageMap": {"en": "hello", "fr": "bonjour"}, "observedAt": "2026-03-01T12:00:00Z"}]}

Delete Fixture Entities
    FOR    ${id}    IN    ${mid}    ${high}    ${numname}    ${arr}    ${lang}
        ${response}=    DELETE    url=${temporal_api_url}/temporal/entities/${id}    expected_status=any
    END
