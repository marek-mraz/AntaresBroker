*** Settings ***
Documentation       Check that entities can be queried with two levels linked entities in different join types and representations

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entities And Linked Entities
Test Teardown       Delete Created Entities And Linked Entities
Test Template       Query Entities With Joins And Representations


*** Variables ***
${building_id_prefix}=                  urn:ngsi-ld:Building:
${city_id_prefix}=                      urn:ngsi-ld:City:
${country_id_prefix}=                   urn:ngsi-ld:Country:
${linking_entity_filename}=             building-relationship.jsonld
${level_1_linked_entity_filename}=      city-relationship.jsonld
${level_2_linked_entity_filename}=      country-minimal.jsonld


*** Test Cases ***    JOIN    OPTIONS    EXPECTATION_FILENAME
019_16_01 Query Inline Normalized
    [Tags]    e-query    5_7_2    4_5_23    since_v1.8.1
    inline    ${EMPTY}    linked-entity-retrieval/buildings-two-levels-inline-019-16.json
019_16_02 Query Flat Normalized
    [Tags]    e-query    5_7_2    4_5_23    since_v1.8.1
    flat    ${EMPTY}    linked-entity-retrieval/buildings-two-levels-flat-019-16.json
019_16_03 Query Inline Simplified
    [Tags]    e-query    5_7_2    4_5_23    since_v1.8.1
    inline    keyValues    linked-entity-retrieval/buildings-two-levels-inline-simplified-019-16.json
019_16_04 Query Flat Simplified
    [Tags]    e-query    5_7_2    4_5_23    since_v1.8.1
    flat    keyValues    linked-entity-retrieval/buildings-two-levels-flat-simplified-019-16.json


*** Keywords ***
Query Entities With Joins And Representations
    [Documentation]    Check that entities can be queried with two levels linked entities in different join types and representations
    [Arguments]    ${join}    ${options}    ${expectation_filename}

    ${response}=    Query Entities
    ...    entity_types=Building
    ...    options=${options}
    ...    join=${join}
    ...    joinLevel=2
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Content
    ...    expectation_filename=${expectation_filename}
    ...    response_body=${response.json()}

Create Initial Entities And Linked Entities
    ${entity_1_id}=    Catenate    ${building_id_prefix}019-16-1
    Set Suite Variable    ${entity_1_id}
    ${linked_entity_1_level_1_id}=    Catenate    ${city_id_prefix}019-16-1
    Set Suite Variable    ${linked_entity_1_level_1_id}
    ${create_response1}=    Create Linking Entity
    ...    entity_filename=${linking_entity_filename}
    ...    entity_id=${entity_1_id}
    ...    linked_entity_id=${linked_entity_1_level_1_id}
    ${linked_entity_1_level_2_id}=    Catenate    ${country_id_prefix}019-16-1
    Set Suite Variable    ${linked_entity_1_level_2_id}
    ${create_response2}=    Create Linking Entity
    ...    entity_filename=${level_1_linked_entity_filename}
    ...    entity_id=${linked_entity_1_level_1_id}
    ...    linked_entity_id=${linked_entity_1_level_2_id}
    Check Response Status Code    201    ${create_response2.status_code}
    ${create_response3}=    Create Entity Selecting Content Type
    ...    filename=${level_2_linked_entity_filename}
    ...    entity_id=${linked_entity_1_level_2_id}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}

    ${entity_2_id}=    Catenate    ${building_id_prefix}019-16-2
    Set Suite Variable    ${entity_2_id}
    ${linked_entity_2_level_1_id}=    Catenate    ${city_id_prefix}019-16-2
    Set Suite Variable    ${linked_entity_2_level_1_id}
    ${create_response4}=    Create Linking Entity
    ...    entity_filename=${linking_entity_filename}
    ...    entity_id=${entity_2_id}
    ...    linked_entity_id=${linked_entity_2_level_1_id}
    ${linked_entity_2_level_2_id}=    Catenate    ${country_id_prefix}019-16-2
    Set Suite Variable    ${linked_entity_2_level_2_id}
    ${create_response5}=    Create Linking Entity
    ...    entity_filename=${level_1_linked_entity_filename}
    ...    entity_id=${linked_entity_2_level_1_id}
    ...    linked_entity_id=${linked_entity_2_level_2_id}
    Check Response Status Code    201    ${create_response2.status_code}
    ${create_response6}=    Create Entity Selecting Content Type
    ...    filename=${level_2_linked_entity_filename}
    ...    entity_id=${linked_entity_2_level_2_id}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}

Delete Created Entities And Linked Entities
    Delete Entity    ${entity_1_id}
    Delete Entity    ${linked_entity_1_level_1_id}
    Delete Entity    ${linked_entity_1_level_2_id}
    Delete Entity    ${entity_2_id}
    Delete Entity    ${linked_entity_2_level_1_id}
    Delete Entity    ${linked_entity_2_level_2_id}

Create Linking Entity
    [Arguments]    ${entity_filename}    ${entity_id}    ${linked_entity_id}
    ${entity_payload}=    Load JSON From File    ${EXECDIR}/data/entities/${entity_filename}
    ${entity}=    Update Value To JSON    ${entity_payload}    $.id    ${entity_id}
    ${entity}=    Update Value To JSON    ${entity}    $.locatedAt.object    ${linked_entity_id}
    ${response}=    Create Entity From JSON-LD Content
    ...    ${entity}
    Check Response Status Code    201    ${response.status_code}
    RETURN    ${response}
