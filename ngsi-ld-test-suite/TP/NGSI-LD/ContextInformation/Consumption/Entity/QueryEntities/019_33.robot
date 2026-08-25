*** Settings ***
Documentation       Check the nesting ceiling of a pick/omit attribute projection
...                 (CIM 009 clause 4.21, applied per clause 5.7.2.4).
...
...                 Normative sources: 4.21 LinkedEntityTerm — each "{...}" selection
...                 addresses the Attributes of the Entity linked by its head, so every
...                 level is one Linked Entity hop; 5.7.2.4 "if the number of hops
...                 ... is bigger than the value of the joinLevel parameter ... it shall
...                 result in a 400 Bad Request Data error"; "the Linked Entity
...                 retrieval ... is only performed if the join parameter is present".
...
...                 Antares extension TP — 019_18 covers the pick/omit grammar
...                 rejections but no nesting depth at all.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Query Entities With A Nested Projection Expecting Bad Request Data


*** Variables ***
${two_hops}=            locatedAt{owner{name}}
${eleven_hops}=         a{a{a{a{a{a{a{a{a{a{a{name}}}}}}}}}}}


*** Test Cases ***    PICK    OMIT    JOIN    JOINLEVEL
019_33_01 Pick Nested Deeper Than The Requested JoinLevel Is Rejected
    [Documentation]    5.7.2.4: a two-hop selection with joinLevel 1 asks for more hops
    ...    than the request allows
    [Tags]    e-query    4_21    5_7_2    since_v1.9.1
    ${two_hops}    ${EMPTY}    inline    1

019_33_02 Omit Nested Deeper Than The Requested JoinLevel Is Rejected
    [Documentation]    5.7.2.4: the hop ceiling applies to omit exactly as to pick
    [Tags]    e-query    4_21    5_7_2    since_v1.9.1
    ${EMPTY}    ${two_hops}    inline    1

019_33_03 Nested Pick Without Join Is Rejected
    [Documentation]    5.7.2.4: "the Linked Entity retrieval ... is only performed if the
    ...    join parameter is present" — a selection that addresses a linked Entity has no
    ...    meaning without it
    [Tags]    e-query    4_21    5_7_2    since_v1.9.1
    ${two_hops}    ${EMPTY}    ${EMPTY}    ${EMPTY}

019_33_04 Pick Nested Past The JoinLevel Ceiling Is Rejected
    [Documentation]    5.7.2.4: joinLevel is bounded, so a selection of eleven hops
    ...    exceeds any admissible joinLevel and is rejected however join is set
    [Tags]    e-query    4_21    5_7_2    since_v1.9.1
    ${eleven_hops}    ${EMPTY}    inline    10

019_33_05 Omit Nested Past The JoinLevel Ceiling Is Rejected
    [Documentation]    5.7.2.4: the same ceiling on the omit spelling
    [Tags]    e-query    4_21    5_7_2    since_v1.9.1
    ${EMPTY}    ${eleven_hops}    inline    10


*** Keywords ***
Query Entities With A Nested Projection Expecting Bad Request Data
    [Arguments]    ${pick}    ${omit}    ${join}    ${join_level}
    ${response}=    Query Entities
    ...    entity_types=Building
    ...    pick=${pick}
    ...    omit=${omit}
    ...    join=${join}
    ...    joinLevel=${join_level}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
