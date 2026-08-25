*** Settings ***
Documentation       Check the EntitySelector data type (CIM 009 clause 5.2.33,
...                 Table 5.2.33-1) on POST /entityOperations/query: id is "String or
...                 String[]" of valid URIs, type is the mandatory selector member, and
...                 "id takes precedence over idPattern".
...
...                 Antares extension TP — the official TPs only exercise single-id
...                 selectors without idPattern.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Setup Selector Entities
Suite Teardown      Delete Selector Entities


*** Variables ***
${vehicle_a}=       urn:ngsi-ld:Vehicle:52330-A
${vehicle_b}=       urn:ngsi-ld:Vehicle:52330-B


*** Test Cases ***
5233_01_01 An Id Array Selects All Named Entities
    [Documentation]    Table 5.2.33-1: id — "String or String[]", valid URI(s)
    [Tags]    e-query    5_2_33    since_v1.9.1
    Post Selector Query Expecting    200
    ...    {"type": "Vehicle", "id": ["urn:ngsi-ld:Vehicle:52330-A", "urn:ngsi-ld:Vehicle:52330-B"]}
    ...    ${vehicle_a}    ${None}

5233_01_02 Id Takes Precedence Over IdPattern
    [Documentation]    5.2.33: "id takes precedence over idPattern"
    [Tags]    e-query    5_2_33    since_v1.9.1
    Post Selector Query Expecting    200
    ...    {"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:52330-A", "idPattern": "^urn:ngsi-ld:Vehicle:52330-B$"}
    ...    ${vehicle_a}    ${None}

5233_01_03 A Selector Without Type Is Rejected
    [Documentation]    Table 5.2.33-1: type is the mandatory selector member
    [Tags]    e-query    5_2_33    since_v1.9.1
    Post Selector Query Expecting    400    {"id": "urn:ngsi-ld:Vehicle:52330-A"}    ${None}    ${None}

5233_01_04 A Non-URI Id Is Rejected
    [Documentation]    Table 5.2.33-1: id restriction "Valid URI(s)"
    [Tags]    e-query    5_2_33    since_v1.9.1
    Post Selector Query Expecting    400    {"type": "Vehicle", "id": "not a uri"}    ${None}    ${None}

5233_01_05 A Mixed-Type Id Array Is Rejected
    [Tags]    e-query    5_2_33    since_v1.9.1
    Post Selector Query Expecting    400
    ...    {"type": "Vehicle", "id": ["urn:ngsi-ld:Vehicle:52330-A", 5]}    ${None}    ${None}

5233_01_06 An IdPattern Alone Still Filters
    [Documentation]    control: without id the pattern applies
    [Tags]    e-query    5_2_33    since_v1.9.1
    Post Selector Query Expecting    200
    ...    {"type": "Vehicle", "idPattern": "^urn:ngsi-ld:Vehicle:52330-B$"}
    ...    ${vehicle_b}    ${vehicle_a}


*** Keywords ***
Post Selector Query Expecting
    [Arguments]    ${expected_status_code}    ${selector_json}    ${must_contain}    ${must_not_contain}
    ${payload}=    Evaluate
    ...    {"type": "Query", "entities": [json.loads('''${selector_json}''')]}    modules=json
    ${response}=    POST
    ...    url=${url}/${ENTITY_OPERATIONS_QUERY_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${{ {"Content-Type": "application/json"} }}
    ...    expected_status=any
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    $must_contain is not None
        Should Contain    ${response.text}    ${must_contain}
    END
    IF    $must_not_contain is not None
        Should Not Contain    ${response.text}    ${must_not_contain}
    END

Setup Selector Entities
    FOR    ${id}    IN    ${vehicle_a}    ${vehicle_b}
        ${payload}=    Evaluate
        ...    json.dumps({"id": $id, "type": "Vehicle", "speed": {"type": "Property", "value": 1}})
        ...    modules=json
        ${response}=    POST
        ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
        ...    data=${payload}
        ...    headers=${{ {"Content-Type": "application/json"} }}
        ...    expected_status=any
        Check Response Status Code    201    ${response.status_code}
    END

Delete Selector Entities
    FOR    ${id}    IN    ${vehicle_a}    ${vehicle_b}
        ${response}=    DELETE
        ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${id}
        ...    expected_status=any
    END
