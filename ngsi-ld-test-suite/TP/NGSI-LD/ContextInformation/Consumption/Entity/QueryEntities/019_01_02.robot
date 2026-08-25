*** Settings ***
Documentation       Check that one can query several entities based on the entities types

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Entities


*** Variables ***
${building_filename}=       building-minimal.jsonld
${vehicle_filename}=        vehicle-simple-attributes.jsonld
${parking_filename}=        parking-simple-attributes.jsonld
${expectation_filename}=    two-types-vehicle-offstreetparking.jsonld
${building_entity_type}=    https://ngsi-ld-test-suite/context#Building
${vehicle_entity_type}=     https://ngsi-ld-test-suite/context#Vehicle
${parking_entity_type}=     https://ngsi-ld-test-suite/context#OffStreetParking


*** Test Cases ***
019_01_02 Query Several Entities Based On The Entities Types
    [Documentation]    Check that one can query several entities based on the entities types
    [Tags]    e-query    5_7_2
    @{entities_ids_to_be_compared}=    Create List    ${vehicle_entity_id}    ${parking_entity_id}
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    ${vehicle_entity_type}    ${parking_entity_type}

    ${response}=    Query Entities    entity_types=${entity_types_to_be_retrieved}    accept=${CONTENT_TYPE_LD_JSON}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Entity Elements With Different Types
    ...    filename=${expectation_filename}
    ...    entities_representation_ids=${entities_ids_to_be_compared}
    ...    response_body=${response.json()}
    ...    ignore_core_context_version=${True}


*** Keywords ***
Setup Initial Entities
    ${building_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${building_entity_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    ${building_filename}
    ...    ${building_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}
    ${vehicle_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${vehicle_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    ${vehicle_filename}
    ...    ${vehicle_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}
    ${parking_entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${parking_entity_id}
    ${create_response3}=    Create Entity Selecting Content Type
    ...    ${parking_filename}
    ...    ${parking_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}

Delete Entities
    Delete Entity    ${building_entity_id}
    Delete Entity    ${vehicle_entity_id}
    Delete Entity    ${parking_entity_id}
