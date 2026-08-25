*** Settings ***
Documentation       Query entities with Entity Type Selection Language.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities
Test Template       Query entities using Entity Type Selection Language


*** Variables ***
${entity_id_prefix}                     urn:ngsi-ld:MultiTypes:
${first_entity_filename}                building-simple-attributes.jsonld
${second_entity_filename}               building-with-different-type.jsonld
${third_entity_filename}                building-with-two-types.jsonld
${building_entity_type}                 Building
${parking_entity_type}                  Parking
${tourist_destination_entity_type}      TouristDestination


*** Test Cases ***    ENTITY_TYPES_SELECTION    EXPECTED_ENTITIES_IDS
019_08_01 Query With One Type    ${building_entity_type}    ${first_entity_id},${third_entity_id}
019_08_02 Query With The AND Operator    (${building_entity_type};${tourist_destination_entity_type})    ${third_entity_id}
019_08_03 Query With The OR Operator    ${building_entity_type},${parking_entity_type}    ${first_entity_id},${second_entity_id},${third_entity_id}
019_08_04 Different Query With The OR Operator    ${parking_entity_type},${tourist_destination_entity_type}    ${second_entity_id},${third_entity_id}
019_08_05 Query With Two Operators    (${building_entity_type};${parking_entity_type}),${tourist_destination_entity_type}    ${third_entity_id}


*** Keywords ***
Query entities using Entity Type Selection Language
    [Documentation]    Query entities with Entity Type Selection Language.
    [Tags]    e-query    4_17    5_7_2    since_v1.5.1
    [Arguments]    ${entity_types_selection}    ${expected_entities_ids}

    ${entities_ids}=    Split String    ${expected_entities_ids}    ,
    @{entities_ids_list}=    Create List    ${entities_ids}
    ${response}=    Query Entities
    ...    entity_types=${entity_types_selection}
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Entities URIS set to
    ...    @{entities_ids_list}
    ...    ${response.json()}

Setup Initial Entities
    ${first_entity_id}=    Generate Random Id    ${entity_id_prefix}
    Set Suite Variable    ${first_entity_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    ${first_entity_filename}
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}
    ${second_entity_id}=    Generate Random Id    ${entity_id_prefix}
    Set Suite Variable    ${second_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    ${second_entity_filename}
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}
    ${third_entity_id}=    Generate Random Id    ${entity_id_prefix}
    Set Suite Variable    ${third_entity_id}
    ${create_response3}=    Create Entity Selecting Content Type
    ...    ${third_entity_filename}
    ...    ${third_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}

Delete Initial Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
    Delete Entity    ${third_entity_id}
