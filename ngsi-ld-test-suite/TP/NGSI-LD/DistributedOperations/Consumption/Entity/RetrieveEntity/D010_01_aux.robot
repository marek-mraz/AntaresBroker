*** Settings ***
Documentation       Verify that when an auxiliary registration exists on a Context Broker and an entity with the same ID exists both locally in the Context Broker and remotely in the Context Source with different attributes, a retrieval request to the Context Broker is correctly forwarded to the Context Source, and the response includes the local entity enriched with additional, non-conflicting attributes from the Context Source

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.json
${entity_payload_filename2}             vehicle-simple-attributes-second.jsonld
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-complete.jsonld
${fragment_filename}                    vehicle-brandname-fragment.json
${expected_attribute}                   brandName


*** Test Cases ***
D010_01_aux Retrieve Entity That Exists On Both The Context Source And The Context Broker From The Context Broker
    [Documentation]    Check that if one retrieves entity living on on both the Context Broker and a Context Source, entities get merged correctly
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-auxiliary    4_3_6_2    5_7_1
    ${entity_body}=    Load Entity    ${entity_payload_filename}    ${entity_id}
    ${entity_fragment}=    Load JSON From File    ${EXECDIR}/data/entities/fragmentEntities/${fragment_filename}
    # `Load Entity` returns a Python dict; HttpCtrl's `Set Stub Reply` writes
    # the body argument verbatim and does NOT auto-serialise non-string values
    # (a dict passed straight in: `len(dict)` reports the key count for the
    # Content-Length header, and `wfile.write(dict)` raises silently — the
    # mock ends up sending headers only). Serialise to a JSON string first.
    ${entity_body_json}=    Evaluate    json.dumps($entity_body)    json
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${entity_id}    200    ${entity_body_json}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing an Attribute set to
    ...    ${expected_attribute}
    ...    ${response.json()}
    ...    ${entity_fragment}
    Should Have Value In Json    ${response.json()}    $.isParked
    Should Have Value In Json    ${response.json()}    $.isParked2


*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity    ${entity_payload_filename2}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    mode=auxiliary
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
