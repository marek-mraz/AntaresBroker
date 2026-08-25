*** Settings ***
Documentation       Verify 4.6.2 Supported names.
...
...                 4.6.2: Entity Type, Property and Relationship names are
...                 restricted to name = unicodeLetter *(unicodeLetter /
...                 unicodeNumber / "_"), optionally prefix:name. "When receiving
...                 a JSON-LD object with a name (Type, Property, Relationship)
...                 including characters different than those expressed above,
...                 implementations should raise an error of type BadRequestData."
...
...                 Antares extension TP — the official suite has zero 4.6.2
...                 coverage (no invalid-name test exists).

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
462_01_01 Attribute Name With Invalid Characters Is Rejected
    [Documentation]    4.6.2: a Property name containing a space (not letter /
    ...    number / underscore) → 400 BadRequestData, and the entity must NOT
    ...    have been created.
    [Tags]    e-create    4_6_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "my attr": {"type": "Property", "value": 1}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}

462_01_02 Entity Type Starting With A Digit Is Rejected
    [Documentation]    4.6.2: name = unicodeLetter *nameChar — a Type starting
    ...    with a digit → 400 BadRequestData.
    [Tags]    e-create    4_6_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "9Vehicle", "@context": [$ngsild_test_suite_context], "speed": {"type": "Property", "value": 1}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

462_01_03 Unicode Letter Names Are Accepted
    [Documentation]    4.6.2: unicodeLetter is any Unicode Letter-category
    ...    character (\p{L}) — a diacritic attribute name and Type are valid and
    ...    round-trip; the response must NOT carry a 4.6.2 rejection.
    [Tags]    e-create    e-retrieve    4_6_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Škola", "@context": [$ngsild_test_suite_context], "výška_1": {"type": "Property", "value": 7}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${value}=    Evaluate    $response.json()['výška_1']['value']
    Should Be Equal As Integers    ${value}    7
    [Teardown]    Delete Entity    ${entity_id}
