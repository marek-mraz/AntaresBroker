*** Settings ***
Documentation       Check that one can query several entities via POST Interaction based on attribute names

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Entities
Suite Teardown      Delete Entities


*** Variables ***
${building_filename}=       building-minimal.jsonld
${vehicle_filename}=        vehicle-simple-attributes.jsonld
${expectation_filename}=    vehicle-simple-attributes-core-context.json
${attribute_brandname}=     https://ngsi-ld-test-suite/context#brandName
${attribute_isparked}=      https://uri.etsi.org/ngsi-ld/default-context/isParked


*** Test Cases ***
019_02_04 Query Several Entities Via POST Interaction Based On Attribute Names
    [Documentation]    Check that one can query several entities via POST Interaction based on attribute names
    [Tags]    e-query    5_7_2
    @{attributes_to_be_retrieved}=    Create List    ${attribute_brandname}    ${attribute_isparked}
    @{entities_ids_to_be_retrieved}=    Create List    ${vehicle_entity_id}
    ${response}=    Query Entities Via POST    attrs=${attributes_to_be_retrieved}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Entity Elements
    ...    ${expectation_filename}
    ...    ${entities_ids_to_be_retrieved}
    ...    ${response.json()}


*** Keywords ***
Create Entities
    ${building_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${building_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${building_filename}
    ...    ${building_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${vehicle_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${vehicle_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${vehicle_filename}
    ...    ${vehicle_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Entities
    Delete Entity    ${building_entity_id}
    Delete Entity    ${vehicle_entity_id}
