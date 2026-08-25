*** Settings ***
Documentation       Verify 5.7.4.3: expandValues on the temporal query — the
...                 4.9 EXAMPLE 12 coercion (query term values expanded
...                 against the @context before executing). With expandValues
...                 the VocabProperty short term matches; without it the same
...                 q must NOT match. Antares extension TP.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections

Test Setup          Create Fixture Entity
Test Teardown       Delete Fixture Entity


*** Variables ***
${shop}=        urn:ngsi-ld:Shop:ev5743
${window}=      timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt


*** Test Cases ***
5743_01_01 ExpandValues Coerces The Temporal Query Term
    [Documentation]    5.7.4.3: q=category==commercial matches the
    ...    VocabProperty only when expandValues=category is present.
    [Tags]    em-usage    5_7_4_3    since_v1.9.1

    ${response}=    GET
    ...    url=${url}/temporal/entities
    ...    params=type=Shop&q=category%3D%3Dcommercial&expandValues=category&${window}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${shop}

5743_01_02 Without ExpandValues The Literal Does Not Match
    [Documentation]    5.7.4.3 / 4.9 EXAMPLE 12: the same q without
    ...    expandValues must NOT return the entity.
    [Tags]    em-usage    5_7_4_3    since_v1.9.1

    ${response}=    GET
    ...    url=${url}/temporal/entities
    ...    params=type=Shop&q=category%3D%3Dcommercial&${window}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    ${shop}


*** Keywords ***
Create Fixture Entity
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${payload}=    Set Variable
    ...    {"id": "${shop}", "type": "Shop", "category": {"type": "VocabProperty", "vocab": "commercial"}}
    ${response}=    POST    url=${url}/entities    data=${payload}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Delete Fixture Entity
    ${response}=    DELETE    url=${url}/entities/${shop}    expected_status=any
    ${response}=    DELETE    url=${url}/temporal/entities/${shop}    expected_status=any
