*** Settings ***
Documentation       Check that entities can be queried with a linked entity in different join types and representations

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entities And Linked Entities
Test Teardown       Delete Created Entities And Linked Entities
Test Template       Query Entities With Joins And Representations


*** Variables ***
${building_id_prefix}=          urn:ngsi-ld:Building:
${city_id_prefix}=              urn:ngsi-ld:City:
${linking_entity_filename}=     building-relationship.jsonld
${linked_entity_filename}=      city-minimal.jsonld


*** Test Cases ***    JOIN    OPTIONS    EXPECTATION_FILENAME
019_13_01 Query Inline Normalized
    [Tags]    e-query    5_7_2    4_5_23    since_v1.8.1
    inline    ${EMPTY}    linked-entity-retrieval/buildings-one-level-inline-019-13.json
019_13_02 Query Flat Normalized
    [Tags]    e-query    5_7_2    4_5_23    since_v1.8.1
    flat    ${EMPTY}    linked-entity-retrieval/buildings-one-level-flat-019-13.json
019_13_03 Query Inline Simplified
    [Tags]    e-query    5_7_2    4_5_23    since_v1.8.1
    inline    keyValues    linked-entity-retrieval/buildings-one-level-inline-simplified-019-13.json
019_13_04 Query Flat Simplified
    [Tags]    e-query    5_7_2    4_5_23    since_v1.8.1
    flat    keyValues    linked-entity-retrieval/buildings-one-level-flat-simplified-019-13.json


*** Keywords ***
Query Entities With Joins And Representations
    [Documentation]    Check that entities can be queried with a linked entity in different join types and representations
    [Arguments]    ${join}    ${options}    ${expectation_filename}

    ${response}=    Query Entities
    ...    entity_types=Building
    ...    options=${options}
    ...    join=${join}
    ...    joinLevel=1
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Content
    ...    expectation_filename=${expectation_filename}
    ...    response_body=${response.json()}

Create Initial Entities And Linked Entities
    ${first_entity_id}=    Catenate    ${building_id_prefix}019-13-1
    ${first_linked_entity_id}=    Catenate    ${city_id_prefix}019-13-1
    Set Suite Variable    ${first_entity_id}
    Set Suite Variable    ${first_linked_entity_id}
    ${create_response1}=    Create Linking Entity
    ...    linking_entity_id=${first_entity_id}
    ...    linked_entity_id=${first_linked_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    filename=${linked_entity_filename}
    ...    entity_id=${first_linked_entity_id}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

    ${second_entity_id}=    Catenate    ${building_id_prefix}019-13-2
    ${second_linked_entity_id}=    Catenate    ${city_id_prefix}019-13-2
    Set Suite Variable    ${second_entity_id}
    Set Suite Variable    ${second_linked_entity_id}
    ${create_response3}=    Create Linking Entity
    ...    linking_entity_id=${second_entity_id}
    ...    linked_entity_id=${second_linked_entity_id}
    ${create_response4}=    Create Entity Selecting Content Type
    ...    filename=${linked_entity_filename}
    ...    entity_id=${second_linked_entity_id}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response4.status_code}

Delete Created Entities And Linked Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${first_linked_entity_id}
    Delete Entity    ${second_entity_id}
    Delete Entity    ${second_linked_entity_id}

Create Linking Entity
    [Arguments]    ${linking_entity_id}    ${linked_entity_id}
    ${entity_payload}=    Load JSON From File    ${EXECDIR}/data/entities/${linking_entity_filename}
    ${entity}=    Update Value To JSON    ${entity_payload}    $.id    ${linking_entity_id}
    ${entity}=    Update Value To JSON    ${entity}    $.locatedAt.object    ${linked_entity_id}
    ${response}=    Create Entity From JSON-LD Content
    ...    ${entity}
    Check Response Status Code    201    ${response.status_code}
    RETURN    ${response}
